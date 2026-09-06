//! Deterministic fixed-step simulation.
//!
//! The simulation owns no I/O, performs no logging, and allocates nothing in
//! the stepping loop. Cylinder state lives in a fixed-size array sized by
//! [`crate::config::validate::MAX_CYLINDERS`]; snapshots allocate once per call,
//! at a batch boundary, never per step.
//!
//! [`Simulation`] deliberately keeps its immutable configuration and its mutable
//! running state in separate fields. That lets the stepper borrow the
//! configuration (which now holds interpolation tables and therefore is not
//! `Copy`) at the same time as it mutates state.

pub mod acoustics;
pub mod brake;
pub mod cylinder;
pub mod driveline;
pub mod egr;
pub mod flow;
pub mod gas;
pub mod governor;
pub mod heat_release;
pub mod heat_transfer;
pub mod ignition_delay;
pub mod injection;
pub mod manifold;
pub mod step;
pub mod torque;
pub mod turbo;

use serde::{Deserialize, Serialize};

use crate::config::validate::{ValidatedConfig, CYCLE_RAD, MAX_CYLINDERS};
use crate::error::{ErrorCode, Result, SimError};
use crate::geometry::SliderCrank;
use crate::rng::Pcg32;
use crate::snapshot::{RunState, SimFault, Snapshot, SNAPSHOT_VERSION};

use acoustics::Acoustics;
use cylinder::{CylinderState, Phase};
use governor::{rad_per_s_to_rpm, rpm_to_rad_per_s, GovernorState};

/// Explicit initial conditions for a deterministic reset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetOptions {
    /// Seed for any stochastic term. There is none yet, but the seed is carried
    /// so cycle-to-cycle variation cannot later be added unseeded.
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
    /// Exhaust gas recirculation enable.
    ///
    /// Defaults on, because the manual states EGR is active across the whole
    /// speed range. Turning it off is a demonstration affordance: it shows the
    /// trade the real calibration is making, since recirculated exhaust costs
    /// power and fuel to buy lower NOx.
    #[serde(default = "default_egr_enabled")]
    pub egr_enabled: bool,
    /// Decompression brake stage, `0` for off through `3`.
    ///
    /// The driver's engine brake switch. Whether the stage actually engages is
    /// decided by [`brake::command`] against the published operating conditions,
    /// not here — asking for a stage is not the same as getting it.
    #[serde(default)]
    pub brake_stage: u8,
    /// Selected gear, `0` for neutral.
    #[serde(default)]
    pub gear: u32,
    /// Road grade in percent. Negative is a descent.
    #[serde(default)]
    pub road_grade_percent: f64,
}

fn default_egr_enabled() -> bool {
    true
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: false,
            ignition: false,
            egr_enabled: true,
            // Brake off, out of gear, on the flat: the driveline and the brake
            // both contribute exactly nothing until asked, which is what keeps
            // every pre-Milestone-4 behaviour identical.
            brake_stage: 0,
            gear: 0,
            road_grade_percent: 0.0,
        }
    }
}

/// Intake manifold state for an engine at rest: ambient air, no burned gas.
fn rest_intake(cfg: &crate::config::EngineConfig) -> manifold::State {
    manifold::State {
        pressure_pa: cfg.air_path.ambient_pressure_pa,
        temperature_k: cfg.air_path.ambient_temperature_k,
        burned_fraction: 0.0,
    }
}

/// Exhaust manifold state for an engine at rest.
///
/// Ambient, and holding air rather than exhaust. A stopped engine has nothing
/// pumping and nothing to hold pressure, so any other starting pressure is not a
/// state the engine can actually be in: the turbine would immediately dump it
/// towards ambient, and that dump is a real transient in the acoustic source —
/// heard as a crack every time the simulation is reset.
///
/// The burned-gas fraction is zero for the same reason. An engine that has not
/// run has air in its exhaust manifold, and starting it at 1.0 would have EGR
/// recirculating "exhaust" that is really air, wrongly starving the first
/// cycles of oxygen.
fn rest_exhaust(cfg: &crate::config::EngineConfig) -> manifold::State {
    manifold::State {
        pressure_pa: cfg.air_path.ambient_pressure_pa,
        temperature_k: cfg.air_path.ambient_temperature_k,
        burned_fraction: 0.0,
    }
}

/// Instantaneous torque terms, refreshed every step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepReport {
    pub torque_gas_nm: f64,
    pub torque_pumping_nm: f64,
    pub torque_friction_nm: f64,
    pub torque_accessory_nm: f64,
    pub torque_starter_nm: f64,
    pub torque_load_nm: f64,
    /// Road load reflected to the crank. Negative on a descent.
    pub torque_driveline_nm: f64,
    pub torque_net_nm: f64,
}

/// Work integrated over the four-stroke cycle in progress, and the results of
/// the cycle that most recently completed.
///
/// Instantaneous torque in a six-cylinder engine swings by more than a thousand
/// newton-metres within a cycle. Every meaningful torque, power, and efficiency
/// figure is therefore a whole-cycle average taken from here.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CycleAverages {
    // Accumulators for the cycle in progress.
    gas_work_j: f64,
    pumping_work_j: f64,
    friction_work_j: f64,
    accessory_work_j: f64,
    fuel_kg: f64,
    duration_s: f64,

    /// Whether at least one cycle has completed since the last reset.
    pub valid: bool,
    pub cycles_completed: u64,
    pub cycle_duration_s: f64,
    /// Closed-period (gross indicated) torque.
    pub gas_torque_nm: f64,
    /// Gas-exchange torque; negative when pumping costs work.
    pub pumping_torque_nm: f64,
    /// Gas plus pumping: net indicated torque.
    pub indicated_torque_nm: f64,
    pub friction_torque_nm: f64,
    pub accessory_torque_nm: f64,
    /// Indicated minus friction and accessories: what reaches the flywheel.
    pub brake_torque_nm: f64,
    pub brake_power_w: f64,
    pub imep_pa: f64,
    pub bmep_pa: f64,
    pub fuel_per_cycle_mg: f64,
    pub bsfc_g_per_kwh: f64,
}

impl CycleAverages {
    fn reset(&mut self) {
        *self = Self::default();
    }

    #[inline]
    fn accumulate(&mut self, report: &StepReport, d_theta_rad: f64, dt_s: f64) {
        self.gas_work_j += report.torque_gas_nm * d_theta_rad;
        self.pumping_work_j += report.torque_pumping_nm * d_theta_rad;
        self.friction_work_j += report.torque_friction_nm * d_theta_rad;
        self.accessory_work_j += report.torque_accessory_nm * d_theta_rad;
        self.duration_s += dt_s;
    }

    #[inline]
    fn add_fuel(&mut self, fuel_kg: f64) {
        self.fuel_kg += fuel_kg;
    }

    /// Close out the cycle and publish the averages.
    fn complete(&mut self, total_displacement_m3: f64) {
        let gas = self.gas_work_j / CYCLE_RAD;
        let pumping = self.pumping_work_j / CYCLE_RAD;
        let friction = self.friction_work_j / CYCLE_RAD;
        let accessory = self.accessory_work_j / CYCLE_RAD;
        let indicated = gas + pumping;
        let brake = indicated - friction - accessory;

        let brake_work_j = brake * CYCLE_RAD;
        let power = if self.duration_s > 0.0 {
            brake_work_j / self.duration_s
        } else {
            0.0
        };

        self.gas_torque_nm = gas;
        self.pumping_torque_nm = pumping;
        self.indicated_torque_nm = indicated;
        self.friction_torque_nm = friction;
        self.accessory_torque_nm = accessory;
        self.brake_torque_nm = brake;
        self.brake_power_w = power;
        self.imep_pa = if total_displacement_m3 > 0.0 {
            (self.gas_work_j + self.pumping_work_j) / total_displacement_m3
        } else {
            0.0
        };
        self.bmep_pa = if total_displacement_m3 > 0.0 {
            brake_work_j / total_displacement_m3
        } else {
            0.0
        };
        self.fuel_per_cycle_mg = self.fuel_kg * 1.0e6;
        // g/kWh: fuel grams divided by brake energy in kilowatt-hours.
        let energy_kwh = power * self.duration_s / 3.6e6;
        self.bsfc_g_per_kwh = if energy_kwh > 0.0 && power > 0.0 {
            (self.fuel_kg * 1000.0) / energy_kwh
        } else {
            0.0
        };
        self.cycle_duration_s = self.duration_s;
        self.cycles_completed += 1;
        self.valid = true;

        self.gas_work_j = 0.0;
        self.pumping_work_j = 0.0;
        self.friction_work_j = 0.0;
        self.accessory_work_j = 0.0;
        self.fuel_kg = 0.0;
        self.duration_s = 0.0;
    }
}

/// All mutable running state. Separated from the configuration so the stepper
/// can hold `&ValidatedConfig` and `&mut SimState` at once.
#[derive(Debug, Clone)]
pub struct SimState {
    pub(crate) cylinders: [CylinderState; MAX_CYLINDERS],
    pub(crate) cylinder_count: usize,

    pub(crate) crank_angle_rad: f64,
    pub(crate) omega_rad_per_s: f64,
    pub(crate) sim_time_s: f64,
    pub(crate) steps_advanced: u64,

    pub(crate) controls: Controls,
    pub(crate) governor: GovernorState,
    pub(crate) rng: Pcg32,

    /// Intake manifold, downstream of the charge-air cooler and the EGR mixer.
    pub(crate) intake: manifold::State,
    /// Exhaust manifold, upstream of the turbine and the EGR branch.
    pub(crate) exhaust: manifold::State,

    pub(crate) turbo_shaft_rad_per_s: f64,
    pub(crate) wastegate_position: f64,
    pub(crate) wastegate_integral: f64,
    pub(crate) egr_valve_position: f64,
    pub(crate) egr_integral: f64,
    pub(crate) egr_rate: f64,

    /// Air-path flows from the most recent step, kg/s.
    pub(crate) compressor_flow_kg_per_s: f64,
    pub(crate) turbine_flow_kg_per_s: f64,
    pub(crate) wastegate_flow_kg_per_s: f64,
    pub(crate) egr_flow_kg_per_s: f64,
    pub(crate) engine_flow_kg_per_s: f64,

    /// Exhaust acoustic source: filters and the produced-sample ring buffer.
    pub(crate) acoustics: Acoustics,

    /// Engine brake and driveline results from the most recent step. Both are
    /// resolved fresh every step from the controls, so neither is integrated
    /// state — they are cached here only so the snapshot can report them.
    pub(crate) brake: brake::Command,
    pub(crate) driveline: driveline::Output,

    pub(crate) fuel_demand_mg: f64,
    pub(crate) last_fuel_charge_mg: f64,
    pub(crate) last_variant: injection::Variant,
    pub(crate) last_injection_pressure_pa: f64,
    pub(crate) last_ignition_delay_rad: f64,
    pub(crate) last_premixed_fraction: f64,

    pub(crate) peak_pressure_pa_cycle: f64,
    pub(crate) peak_pressure_pa_prev_cycle: f64,
    pub(crate) peak_pressure_pa_session: f64,
    pub(crate) peak_temperature_k_cycle: f64,
    pub(crate) peak_temperature_k_prev_cycle: f64,

    pub(crate) report: StepReport,
    pub(crate) cycle: CycleAverages,
    pub(crate) fault: Option<SimError>,
}

/// Deterministic engine simulation for one validated configuration.
#[derive(Debug, Clone)]
pub struct Simulation {
    config: ValidatedConfig,
    slider: SliderCrank,
    state: SimState,
}

impl Simulation {
    /// Create a simulation and apply a deterministic reset.
    pub fn new(config: ValidatedConfig, options: ResetOptions) -> Result<Self> {
        let slider =
            SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m);
        let cylinder_count = config.config().geometry.cylinders;
        let state = SimState {
            cylinders: [CylinderState::new(0.0, 1.0, 1.0, 0.0); MAX_CYLINDERS],
            cylinder_count,
            crank_angle_rad: 0.0,
            omega_rad_per_s: 0.0,
            sim_time_s: 0.0,
            steps_advanced: 0,
            controls: Controls::default(),
            governor: GovernorState::default(),
            rng: Pcg32::seed_from(options.seed),
            intake: rest_intake(config.config()),
            exhaust: rest_exhaust(config.config()),
            turbo_shaft_rad_per_s: 0.0,
            wastegate_position: 0.0,
            wastegate_integral: 0.0,
            egr_valve_position: 0.0,
            egr_integral: 0.0,
            egr_rate: 0.0,
            compressor_flow_kg_per_s: 0.0,
            turbine_flow_kg_per_s: 0.0,
            wastegate_flow_kg_per_s: 0.0,
            egr_flow_kg_per_s: 0.0,
            engine_flow_kg_per_s: 0.0,
            acoustics: Acoustics::new(config.config().solver.max_steps_per_batch as usize),
            brake: brake::Command::default(),
            driveline: driveline::Output::default(),
            fuel_demand_mg: 0.0,
            last_fuel_charge_mg: 0.0,
            last_variant: injection::Variant::Standard,
            last_injection_pressure_pa: 0.0,
            last_ignition_delay_rad: 0.0,
            last_premixed_fraction: 0.0,
            peak_pressure_pa_cycle: 0.0,
            peak_pressure_pa_prev_cycle: 0.0,
            peak_pressure_pa_session: 0.0,
            peak_temperature_k_cycle: 0.0,
            peak_temperature_k_prev_cycle: 0.0,
            report: StepReport::default(),
            cycle: CycleAverages::default(),
            fault: None,
        };
        let mut sim = Self {
            config,
            slider,
            state,
        };
        sim.reset(options)?;
        Ok(sim)
    }

    /// The configuration this simulation was built from.
    pub fn config(&self) -> &ValidatedConfig {
        &self.config
    }

    /// Read-only access to running state, for the dyno harness and tests.
    pub fn state(&self) -> &SimState {
        &self.state
    }

    /// Deterministic reset.
    ///
    /// Clears crank state, governor integrator, faults, peak-pressure history,
    /// cycle averages, and controls, reseeds the generator, returns manifold
    /// pressure to ambient, and refills every cylinder with charge at its own
    /// cycle phase. Two resets with identical options always produce identical
    /// state.
    pub fn reset(&mut self, options: ResetOptions) -> Result<()> {
        options.validate()?;

        let cfg = self.config.config();
        let derived = self.config.derived();
        let state = &mut self.state;

        state.crank_angle_rad = wrap_cycle(options.initial_crank_rad);
        state.omega_rad_per_s = rpm_to_rad_per_s(options.initial_rpm);
        state.sim_time_s = 0.0;
        state.steps_advanced = 0;
        state.controls = Controls::default();
        state.governor.reset();
        state.rng = Pcg32::seed_from(options.seed);
        // A reset engine is a cold one: manifolds at rest, turbo stopped, both
        // actuators shut and their integrators released.
        state.intake = rest_intake(cfg);
        state.exhaust = rest_exhaust(cfg);
        state.turbo_shaft_rad_per_s = 0.0;
        state.wastegate_position = 0.0;
        state.wastegate_integral = 0.0;
        state.egr_valve_position = 0.0;
        state.egr_integral = 0.0;
        state.egr_rate = 0.0;
        state.compressor_flow_kg_per_s = 0.0;
        state.turbine_flow_kg_per_s = 0.0;
        state.wastegate_flow_kg_per_s = 0.0;
        state.egr_flow_kg_per_s = 0.0;
        state.engine_flow_kg_per_s = 0.0;
        state.acoustics.reset();
        state.brake = brake::Command::default();
        state.driveline = driveline::Output::default();
        state.fuel_demand_mg = 0.0;
        state.last_fuel_charge_mg = 0.0;
        state.last_variant = injection::Variant::Standard;
        state.last_injection_pressure_pa = 0.0;
        state.last_ignition_delay_rad = 0.0;
        state.last_premixed_fraction = 0.0;
        state.peak_pressure_pa_cycle = 0.0;
        state.peak_pressure_pa_prev_cycle = 0.0;
        state.peak_pressure_pa_session = 0.0;
        state.peak_temperature_k_cycle = 0.0;
        state.peak_temperature_k_prev_cycle = 0.0;
        state.report = StepReport::default();
        state.cycle.reset();
        state.fault = None;
        state.cylinder_count = cfg.geometry.cylinders;

        let pressure = state.intake.pressure_pa;
        let temperature = state.intake.temperature_k;

        for index in 0..MAX_CYLINDERS {
            let offset = derived.phase_offsets_rad.get(index).copied().unwrap_or(0.0);
            let psi = cylinder::signed_cycle_angle(state.crank_angle_rad, offset);
            let volume = self.slider.volume_m3(psi);
            let mass = gas::mass_kg(&cfg.gas, pressure, temperature, volume);
            let mut c = CylinderState::new(offset, pressure, temperature, mass);
            c.phase = cylinder::phase_at(
                psi,
                cfg.valvetrain.intake_valve_close_rad,
                cfg.valvetrain.exhaust_valve_open_rad,
            );
            state.cylinders[index] = c;
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
        if controls.brake_stage > brake::MAX_STAGE {
            return Err(SimError::invalid_control(format!(
                "brake_stage must fall in [0, {}], got {}",
                brake::MAX_STAGE,
                controls.brake_stage
            )));
        }
        let driveline = &self.config.config().driveline;
        if controls.gear > driveline.gear_count() {
            return Err(SimError::invalid_control(format!(
                "gear must fall in [0, {}], got {}",
                driveline.gear_count(),
                controls.gear
            )));
        }
        if !controls.road_grade_percent.is_finite() {
            return Err(SimError::invalid_control(
                "road_grade_percent must be finite",
            ));
        }
        let max_grade = driveline.max_grade_percent;
        if controls.road_grade_percent.abs() > max_grade {
            return Err(SimError::invalid_control(format!(
                "road_grade_percent must fall in [-{max_grade}, {max_grade}], got {}",
                controls.road_grade_percent
            )));
        }
        self.state.controls = controls;
        Ok(())
    }

    /// The engine brake command resolved on the most recent step.
    pub fn brake_command(&self) -> brake::Command {
        self.state.brake
    }

    /// The driveline contribution resolved on the most recent step.
    pub fn driveline_output(&self) -> driveline::Output {
        self.state.driveline
    }

    /// Current controls.
    pub fn controls(&self) -> Controls {
        self.state.controls
    }

    /// Pin the crankshaft to a fixed speed.
    ///
    /// This is the dynamometer affordance. A real engine dyno holds speed
    /// externally and measures the torque needed to do so; pinning here does the
    /// same. It does not change how torque is produced — cylinder pressure,
    /// friction, and the accessory terms are untouched — it only suppresses
    /// crank acceleration so a steady-state point settles in a bounded number of
    /// cycles instead of depending on a tuned load controller.
    ///
    /// The live simulation never calls this; only [`crate::dyno`] does.
    pub fn pin_speed_rpm(&mut self, rpm: f64) -> Result<()> {
        if !rpm.is_finite() || rpm < 0.0 {
            return Err(SimError::invalid_control(
                "pinned speed must be finite and not negative",
            ));
        }
        let max_rpm = self.config.config().limits.max_rpm;
        if rpm > max_rpm {
            return Err(SimError::invalid_control(format!(
                "pinned speed {rpm} exceeds the configured {max_rpm} rpm limit"
            )));
        }
        self.state.omega_rad_per_s = rpm_to_rad_per_s(rpm);
        Ok(())
    }

    /// Advance exactly `steps` fixed steps.
    ///
    /// `advance(n)` is bit-identical to `n` calls of `advance(1)`. Once a fault
    /// is latched, every further call returns that fault and state stops moving.
    pub fn advance(&mut self, steps: u32) -> Result<()> {
        if let Some(fault) = &self.state.fault {
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
            if let Err(error) = step::step(&self.config, &self.slider, &mut self.state) {
                self.state.fault = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    /// Engine speed in rpm.
    pub fn rpm(&self) -> f64 {
        rad_per_s_to_rpm(self.state.omega_rad_per_s)
    }

    /// Sample rate of the exhaust audio, in hertz.
    ///
    /// One sample per solver step, so this is just the reciprocal of the fixed
    /// step: 40 kHz at 25 us. Consumers resample from here to whatever rate
    /// their audio device runs at.
    pub fn audio_sample_rate_hz(&self) -> f64 {
        1.0 / self.config.config().solver.fixed_step_s
    }

    /// Copy produced audio samples into `out`, returning how many were written.
    ///
    /// Samples are produced one per step regardless of whether anyone drains
    /// them; a caller that never drains simply loses the oldest. Draining once
    /// per batch, which is how the worker uses this, never loses any because the
    /// buffer is sized for a whole batch.
    pub fn drain_audio(&mut self, out: &mut [f32]) -> usize {
        self.state.acoustics.drain(out)
    }

    /// Samples waiting to be drained.
    pub fn audio_available(&self) -> usize {
        self.state.acoustics.available()
    }

    /// Samples lost because the consumer fell behind since the last reset.
    pub fn audio_dropped(&self) -> u64 {
        self.state.acoustics.dropped()
    }

    /// Exhaust level in decibels, relative to the configured reference.
    pub fn audio_level_db(&self) -> f64 {
        self.state
            .acoustics
            .level_db(self.config.config().audio.reference_spl_db)
    }

    /// Cycle-averaged results of the most recently completed cycle.
    pub fn cycle_averages(&self) -> CycleAverages {
        self.state.cycle
    }

    /// Coarse run state.
    pub fn run_state(&self) -> RunState {
        if self.state.fault.is_some() {
            return RunState::Fault;
        }
        let rpm = self.rpm();
        if self.state.controls.ignition && rpm >= self.config.config().load.starter_cutout_rpm {
            RunState::Running
        } else if self.state.controls.starter || rpm > 1.0 {
            RunState::Cranking
        } else {
            RunState::Stopped
        }
    }

    /// Mean fresh air trapped per cylinder at the last intake valve closing, kg.
    pub fn mean_trapped_air_kg(&self) -> f64 {
        let count = self.state.cylinder_count;
        if count == 0 {
            return 0.0;
        }
        let sum: f64 = self.state.cylinders[..count]
            .iter()
            .map(|c| c.trapped_air_kg)
            .sum();
        sum / count as f64
    }

    /// Mean residual-gas fraction across the cylinders.
    pub fn residual_fraction(&self) -> f64 {
        let count = self.state.cylinder_count;
        if count == 0 {
            return 0.0;
        }
        let sum: f64 = self.state.cylinders[..count]
            .iter()
            .map(CylinderState::residual_fraction)
            .sum();
        sum / count as f64
    }

    /// Build a compact telemetry snapshot. Allocates once; call at batch
    /// boundaries, not per step.
    pub fn snapshot(&self) -> Snapshot {
        let cfg = self.config.config();
        let state = &self.state;
        let cycle = state.cycle;

        let mut cylinder_pressure_pa = Vec::with_capacity(state.cylinder_count);
        let mut cylinder_motored_pressure_pa = Vec::with_capacity(state.cylinder_count);
        for c in &state.cylinders[..state.cylinder_count] {
            cylinder_pressure_pa.push(c.pressure_pa);
            cylinder_motored_pressure_pa.push(c.motored_pressure_pa);
        }

        Snapshot {
            schema_version: SNAPSHOT_VERSION,
            config_id: cfg.identity.id.clone(),
            state: self.run_state(),
            sim_time_s: state.sim_time_s,
            crank_angle_rad: state.crank_angle_rad,
            rpm: self.rpm(),
            steps_advanced: state.steps_advanced,

            cylinder_pressure_pa,
            cylinder_motored_pressure_pa,
            peak_pressure_pa_cycle: state
                .peak_pressure_pa_cycle
                .max(state.peak_pressure_pa_prev_cycle),
            peak_pressure_pa_session: state.peak_pressure_pa_session,
            peak_gas_temperature_k: state
                .peak_temperature_k_cycle
                .max(state.peak_temperature_k_prev_cycle),

            torque_gas_nm: state.report.torque_gas_nm,
            torque_pumping_nm: state.report.torque_pumping_nm,
            torque_friction_nm: state.report.torque_friction_nm,
            torque_accessory_nm: state.report.torque_accessory_nm,
            torque_starter_nm: state.report.torque_starter_nm,
            torque_load_nm: state.report.torque_load_nm,
            torque_net_nm: state.report.torque_net_nm,

            cycle_valid: cycle.valid,
            cycles_completed: cycle.cycles_completed,
            indicated_torque_cycle_nm: cycle.indicated_torque_nm,
            brake_torque_cycle_nm: cycle.brake_torque_nm,
            brake_power_cycle_w: cycle.brake_power_w,
            imep_pa: cycle.imep_pa,
            bmep_pa: cycle.bmep_pa,
            bsfc_g_per_kwh: cycle.bsfc_g_per_kwh,

            fuel_per_cycle_mg: state.last_fuel_charge_mg,
            fuel_demand_mg: state.fuel_demand_mg,
            injection_variant: state.last_variant.as_str().to_string(),
            injection_pressure_pa: state.last_injection_pressure_pa,
            ignition_delay_rad: state.last_ignition_delay_rad,
            premixed_fraction: state.last_premixed_fraction,

            intake_pressure_pa: state.intake.pressure_pa,
            intake_temperature_k: state.intake.temperature_k,
            exhaust_pressure_pa: state.exhaust.pressure_pa,
            exhaust_temperature_k: state.exhaust.temperature_k,
            residual_fraction: self.residual_fraction(),

            boost_pressure_pa: (state.intake.pressure_pa
                - self.config.config().air_path.ambient_pressure_pa)
                .max(0.0),
            turbo_shaft_rad_per_s: state.turbo_shaft_rad_per_s,
            wastegate_position: state.wastegate_position,
            egr_rate: state.egr_rate,
            egr_valve_position: state.egr_valve_position,
            intake_burned_fraction: state.intake.burned_fraction,
            compressor_flow_kg_per_s: state.compressor_flow_kg_per_s,
            turbine_flow_kg_per_s: state.turbine_flow_kg_per_s,
            egr_flow_kg_per_s: state.egr_flow_kg_per_s,

            brake_stage_active: state.brake.stage,
            brake_active: state.brake.active,
            // Absorbed power is the cycle average with its sign turned round, and
            // only while the brake is actually operating. A coasting engine also
            // shows negative brake power — that is just friction and pumping —
            // and reporting it as brake output would overstate what the brake is
            // doing.
            brake_absorbed_power_w: if state.brake.active {
                (-cycle.brake_power_w).max(0.0)
            } else {
                0.0
            },
            vehicle_speed_m_per_s: state.driveline.vehicle_speed_m_per_s,
            torque_driveline_nm: state.report.torque_driveline_nm,
            reflected_inertia_kg_m2: state.driveline.reflected_inertia_kg_m2,
            gear_engaged: state.driveline.engaged,

            audio_level_db: self.audio_level_db(),

            fault: state.fault.as_ref().map(SimFault::from),
        }
    }

    /// Phase of a cylinder, for tests and diagnostics.
    pub fn cylinder_phase(&self, index: usize) -> Option<Phase> {
        self.state
            .cylinders
            .get(index)
            .filter(|_| index < self.state.cylinder_count)
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
