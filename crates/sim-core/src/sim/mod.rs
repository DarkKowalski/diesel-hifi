//! Deterministic fixed-step simulation.
//!
//! The simulation owns no I/O, performs no logging, and allocates nothing in
//! the stepping loop. Cylinder state lives in a fixed-size array sized by
//! [`crate::config::validate::MAX_CYLINDERS`]; snapshots allocate once per call,
//! at a batch boundary, never per step.

pub mod cylinder;
pub mod governor;
pub mod heat_release;
pub mod step;
pub mod torque;

use serde::{Deserialize, Serialize};

use crate::config::validate::{ValidatedConfig, CYCLE_RAD, MAX_CYLINDERS};
use crate::error::{ErrorCode, Result, SimError};
use crate::geometry::SliderCrank;
use crate::rng::Pcg32;
use crate::snapshot::{RunState, SimFault, Snapshot, SNAPSHOT_VERSION};

use cylinder::{CylinderState, Phase};
use governor::{rad_per_s_to_rpm, rpm_to_rad_per_s, GovernorState};

/// Explicit initial conditions for a deterministic reset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetOptions {
    /// Seed for any stochastic term. Milestone 1 has none, but the seed is
    /// carried so that cycle-to-cycle variation cannot later be added unseeded.
    pub seed: u64,
    pub initial_rpm: f64,
    pub initial_crank_rad: f64,
    pub coolant_temp_k: f64,
}

impl Default for ResetOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            initial_rpm: 0.0,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        }
    }
}

impl ResetOptions {
    fn validate(&self) -> Result<()> {
        if !self.initial_rpm.is_finite()
            || !self.initial_crank_rad.is_finite()
            || !self.coolant_temp_k.is_finite()
        {
            return Err(SimError::invalid_control(
                "reset options must be finite numbers",
            ));
        }
        if self.initial_rpm < 0.0 {
            return Err(SimError::invalid_control(
                "initial_rpm must not be negative",
            ));
        }
        if self.coolant_temp_k <= 0.0 {
            return Err(SimError::invalid_control(
                "coolant_temp_k must be above absolute zero",
            ));
        }
        Ok(())
    }
}

/// Driver and load inputs.
///
/// `pedal` requests fuel. It is not a gasoline throttle plate: it never chokes
/// the intake and never becomes a torque target.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Controls {
    /// Fuel request, `0..=1`.
    pub pedal: f64,
    /// External resisting torque, N m.
    pub load_torque_nm: f64,
    /// Starter motor engagement request.
    pub starter: bool,
    /// Fuelling enable. Clearing it shuts the engine down.
    pub ignition: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: false,
            ignition: false,
        }
    }
}

/// Per-batch torque and pressure bookkeeping, refreshed every step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct StepReport {
    pub torque_gas_nm: f64,
    pub torque_pumping_nm: f64,
    pub torque_friction_nm: f64,
    pub torque_accessory_nm: f64,
    pub torque_starter_nm: f64,
    pub torque_load_nm: f64,
    pub torque_net_nm: f64,
}

/// Deterministic engine simulation for one validated configuration.
#[derive(Debug, Clone)]
pub struct Simulation {
    pub(crate) config: ValidatedConfig,
    pub(crate) slider: SliderCrank,
    pub(crate) cylinders: [CylinderState; MAX_CYLINDERS],
    pub(crate) cylinder_count: usize,

    pub(crate) crank_angle_rad: f64,
    pub(crate) omega_rad_per_s: f64,
    pub(crate) sim_time_s: f64,
    pub(crate) steps_advanced: u64,

    pub(crate) controls: Controls,
    pub(crate) governor: GovernorState,
    pub(crate) rng: Pcg32,

    pub(crate) fuel_demand_mg: f64,
    pub(crate) last_fuel_charge_mg: f64,
    pub(crate) peak_pressure_pa_cycle: f64,
    pub(crate) peak_pressure_pa_prev_cycle: f64,
    pub(crate) peak_pressure_pa_session: f64,
    pub(crate) peak_temperature_k_cycle: f64,
    pub(crate) peak_temperature_k_prev_cycle: f64,

    pub(crate) report: StepReport,
    pub(crate) fault: Option<SimError>,
}

impl Simulation {
    /// Create a simulation and apply a deterministic reset.
    pub fn new(config: ValidatedConfig, options: ResetOptions) -> Result<Self> {
        let slider =
            SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m);
        let mut sim = Self {
            slider,
            cylinders: [CylinderState::new(0.0, 0.0, 1.0, 0.0); MAX_CYLINDERS],
            cylinder_count: config.config().geometry.cylinders,
            crank_angle_rad: 0.0,
            omega_rad_per_s: 0.0,
            sim_time_s: 0.0,
            steps_advanced: 0,
            controls: Controls::default(),
            governor: GovernorState::default(),
            rng: Pcg32::seed_from(options.seed),
            fuel_demand_mg: 0.0,
            last_fuel_charge_mg: 0.0,
            peak_pressure_pa_cycle: 0.0,
            peak_pressure_pa_prev_cycle: 0.0,
            peak_pressure_pa_session: 0.0,
            peak_temperature_k_cycle: 0.0,
            peak_temperature_k_prev_cycle: 0.0,
            report: StepReport::default(),
            fault: None,
            config,
        };
        sim.reset(options)?;
        Ok(sim)
    }

    /// The configuration this simulation was built from.
    pub fn config(&self) -> &ValidatedConfig {
        &self.config
    }

    /// Deterministic reset.
    ///
    /// Clears crank state, governor integrator, faults, peak-pressure history,
    /// and controls, reseeds the generator, and refills every cylinder with
    /// intake-manifold gas at its own cycle phase. Two resets with identical
    /// options always produce identical state.
    pub fn reset(&mut self, options: ResetOptions) -> Result<()> {
        options.validate()?;

        let cfg = self.config.config();
        let derived = self.config.derived();

        self.crank_angle_rad = wrap_cycle(options.initial_crank_rad);
        self.omega_rad_per_s = rpm_to_rad_per_s(options.initial_rpm);
        self.sim_time_s = 0.0;
        self.steps_advanced = 0;
        self.controls = Controls::default();
        self.governor.reset();
        self.rng = Pcg32::seed_from(options.seed);
        self.fuel_demand_mg = 0.0;
        self.last_fuel_charge_mg = 0.0;
        self.peak_pressure_pa_cycle = 0.0;
        self.peak_pressure_pa_prev_cycle = 0.0;
        self.peak_pressure_pa_session = 0.0;
        self.peak_temperature_k_cycle = 0.0;
        self.peak_temperature_k_prev_cycle = 0.0;
        self.report = StepReport::default();
        self.fault = None;
        self.cylinder_count = cfg.geometry.cylinders;

        let pressure = cfg.air_path.intake_manifold_pressure_pa;
        let temperature = cfg.air_path.intake_manifold_temperature_k;
        let r = cfg.air_path.gas_constant_j_per_kg_k;

        for index in 0..MAX_CYLINDERS {
            let offset = derived.phase_offsets_rad.get(index).copied().unwrap_or(0.0);
            let psi = cylinder::signed_cycle_angle(self.crank_angle_rad, offset);
            let volume = self.slider.volume_m3(psi);
            let mass = pressure * volume / (r * temperature);
            let mut state = CylinderState::new(offset, pressure, temperature, mass);
            state.phase = cylinder::phase_at(
                psi,
                cfg.valvetrain.intake_valve_close_rad,
                cfg.valvetrain.exhaust_valve_open_rad,
            );
            self.cylinders[index] = state;
        }

        Ok(())
    }

    /// Submit control inputs. Rejects non-finite or out-of-range values.
    pub fn set_controls(&mut self, controls: Controls) -> Result<()> {
        if !controls.pedal.is_finite() || !controls.load_torque_nm.is_finite() {
            return Err(SimError::invalid_control(
                "pedal and load_torque_nm must be finite",
            ));
        }
        if !(0.0..=1.0).contains(&controls.pedal) {
            return Err(SimError::invalid_control(format!(
                "pedal must fall in [0, 1], got {}",
                controls.pedal
            )));
        }
        let max_load = self.config.config().load.max_external_load_nm;
        if !(0.0..=max_load).contains(&controls.load_torque_nm) {
            return Err(SimError::invalid_control(format!(
                "load_torque_nm must fall in [0, {max_load}], got {}",
                controls.load_torque_nm
            )));
        }
        self.controls = controls;
        Ok(())
    }

    /// Current controls.
    pub fn controls(&self) -> Controls {
        self.controls
    }

    /// Advance exactly `steps` fixed steps.
    ///
    /// `advance(n)` is bit-identical to `n` calls of `advance(1)`. Once a fault
    /// is latched, every further call returns that fault and state stops moving.
    pub fn advance(&mut self, steps: u32) -> Result<()> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        let limit = self.config.config().solver.max_steps_per_batch;
        if steps > limit {
            return Err(SimError::new(
                ErrorCode::StepLimitExceeded,
                format!("requested {steps} steps, configured limit is {limit}"),
            ));
        }
        for _ in 0..steps {
            if let Err(error) = step::step(self) {
                self.fault = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    /// Engine speed in rpm.
    pub fn rpm(&self) -> f64 {
        rad_per_s_to_rpm(self.omega_rad_per_s)
    }

    /// Coarse run state.
    pub fn run_state(&self) -> RunState {
        if self.fault.is_some() {
            return RunState::Fault;
        }
        let rpm = self.rpm();
        if self.controls.ignition && rpm >= self.config.config().load.starter_cutout_rpm {
            RunState::Running
        } else if self.controls.starter || rpm > 1.0 {
            RunState::Cranking
        } else {
            RunState::Stopped
        }
    }

    /// Build a compact telemetry snapshot. Allocates once; call at batch
    /// boundaries, not per step.
    pub fn snapshot(&self) -> Snapshot {
        let cfg = self.config.config();
        let mut cylinder_pressure_pa = Vec::with_capacity(self.cylinder_count);
        for state in &self.cylinders[..self.cylinder_count] {
            cylinder_pressure_pa.push(state.pressure_pa);
        }
        Snapshot {
            schema_version: SNAPSHOT_VERSION,
            config_id: cfg.identity.id.clone(),
            state: self.run_state(),
            sim_time_s: self.sim_time_s,
            crank_angle_rad: self.crank_angle_rad,
            rpm: self.rpm(),
            steps_advanced: self.steps_advanced,
            cylinder_pressure_pa,
            peak_pressure_pa_cycle: self
                .peak_pressure_pa_cycle
                .max(self.peak_pressure_pa_prev_cycle),
            peak_pressure_pa_session: self.peak_pressure_pa_session,
            torque_gas_nm: self.report.torque_gas_nm,
            torque_pumping_nm: self.report.torque_pumping_nm,
            torque_friction_nm: self.report.torque_friction_nm,
            torque_accessory_nm: self.report.torque_accessory_nm,
            torque_starter_nm: self.report.torque_starter_nm,
            torque_load_nm: self.report.torque_load_nm,
            torque_net_nm: self.report.torque_net_nm,
            fuel_per_cycle_mg: self.last_fuel_charge_mg,
            fuel_demand_mg: self.fuel_demand_mg,
            intake_pressure_pa: cfg.air_path.intake_manifold_pressure_pa,
            exhaust_pressure_pa: cfg.air_path.exhaust_manifold_pressure_pa,
            peak_gas_temperature_k: self
                .peak_temperature_k_cycle
                .max(self.peak_temperature_k_prev_cycle),
            fault: self.fault.as_ref().map(SimFault::from),
        }
    }

    /// Phase of a cylinder, for tests and diagnostics.
    pub fn cylinder_phase(&self, index: usize) -> Option<Phase> {
        self.cylinders
            .get(index)
            .filter(|_| index < self.cylinder_count)
            .map(|c| c.phase)
    }
}

/// Wrap an angle into `[0, 4*PI)`.
#[inline]
pub fn wrap_cycle(angle: f64) -> f64 {
    let wrapped = angle % CYCLE_RAD;
    if wrapped < 0.0 {
        wrapped + CYCLE_RAD
    } else {
        wrapped
    }
}
