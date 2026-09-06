//! Steady-state dynamometer harness.
//!
//! Holds the engine at a target speed with a proportional load controller, lets
//! it settle, then reports whole-cycle averages. This is how the Milestone 2
//! calibration is measured against the published 375 kW and 2500 N m magnitudes.
//!
//! The controller is a pure function of state, so a sweep is exactly as
//! deterministic as the solver underneath it. Reported torque is taken from the
//! cycle averages, not from the controller's output, so the number does not
//! depend on how well the controller converged.
//!
//! Nothing here is engine-specific: a sweep reads whatever [`ValidatedConfig`]
//! it is handed.

use serde::{Deserialize, Serialize};

use crate::config::ValidatedConfig;
use crate::error::{Result, SimError};
use crate::sim::governor::rpm_to_rad_per_s;
use crate::sim::{Controls, ResetOptions, Simulation};

/// One measured steady-state operating point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatingPoint {
    pub rpm: f64,
    pub pedal: f64,
    pub brake_torque_nm: f64,
    pub indicated_torque_nm: f64,
    pub friction_torque_nm: f64,
    pub pumping_torque_nm: f64,
    pub brake_power_w: f64,
    pub imep_pa: f64,
    pub bmep_pa: f64,
    pub fuel_mg_per_cycle: f64,
    pub bsfc_g_per_kwh: f64,
    pub peak_pressure_pa: f64,
    pub peak_gas_temperature_k: f64,
    pub air_fuel_ratio: f64,
    pub intake_pressure_pa: f64,
    pub exhaust_pressure_pa: f64,
    pub turbo_shaft_rad_per_s: f64,
    pub wastegate_position: f64,
    pub egr_rate: f64,
    pub residual_fraction: f64,
    /// Whether the speed controller actually held the target.
    pub converged: bool,
}

/// Parameters for a speed sweep.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepOptions {
    pub start_rpm: f64,
    pub end_rpm: f64,
    pub step_rpm: f64,
    /// Pedal position held throughout. Use 1.0 for a full-load curve.
    pub pedal: f64,
    /// Cycles allowed for the engine and boost to settle before measuring.
    pub settle_cycles: u32,
    /// Cycles averaged into the reported figures.
    pub measure_cycles: u32,
    /// Whether exhaust gas recirculation runs during the sweep.
    ///
    /// Defaults on, matching how the real engine is calibrated. Sweeping with it
    /// off is how the cost of recirculation is measured rather than asserted.
    #[serde(default = "default_true")]
    pub egr_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            start_rpm: 600.0,
            end_rpm: 2000.0,
            step_rpm: 100.0,
            pedal: 1.0,
            settle_cycles: 40,
            measure_cycles: 12,
            egr_enabled: true,
        }
    }
}

impl SweepOptions {
    fn validate(&self) -> Result<()> {
        if !self.start_rpm.is_finite() || !self.end_rpm.is_finite() || !self.step_rpm.is_finite() {
            return Err(SimError::invalid_control("sweep bounds must be finite"));
        }
        if self.start_rpm <= 0.0 || self.end_rpm < self.start_rpm || self.step_rpm <= 0.0 {
            return Err(SimError::invalid_control(
                "sweep needs 0 < start_rpm <= end_rpm and a positive step",
            ));
        }
        if !(0.0..=1.0).contains(&self.pedal) {
            return Err(SimError::invalid_control("sweep pedal must fall in [0, 1]"));
        }
        if self.settle_cycles == 0 || self.measure_cycles == 0 {
            return Err(SimError::invalid_control(
                "sweep needs at least one settle and one measure cycle",
            ));
        }
        let points = ((self.end_rpm - self.start_rpm) / self.step_rpm).floor() as usize + 1;
        if points > 200 {
            return Err(SimError::invalid_control(
                "sweep would produce more than 200 points; increase step_rpm",
            ));
        }
        Ok(())
    }

    /// Speeds this sweep will visit.
    pub fn speeds(&self) -> Vec<f64> {
        let mut out = Vec::new();
        let mut rpm = self.start_rpm;
        while rpm <= self.end_rpm + 1.0e-9 {
            out.push(rpm);
            rpm += self.step_rpm;
        }
        out
    }
}

/// The crank speed is re-pinned this many times per four-stroke cycle, which
/// holds it flat to a small fraction of a percent within the cycle.
const PINS_PER_CYCLE: u32 = 36;

/// Cycle-to-cycle brake-torque spread, as a fraction, below which a point counts
/// as settled.
const CONVERGENCE_TOLERANCE: f64 = 0.02;

/// Measure one steady-state operating point.
///
/// The crankshaft is held at `rpm` throughout, exactly as an engine dynamometer
/// holds speed and measures the torque required to do so. Torque still comes
/// only from cylinder pressure and the explicit friction and accessory terms;
/// pinning the speed suppresses crank acceleration so the point settles in a
/// bounded number of cycles instead of depending on a tuned load controller.
pub fn operating_point(
    config: &ValidatedConfig,
    rpm: f64,
    pedal: f64,
    settle_cycles: u32,
    measure_cycles: u32,
    egr_enabled: bool,
) -> Result<OperatingPoint> {
    if !rpm.is_finite() || rpm <= 0.0 {
        return Err(SimError::invalid_control(
            "operating point rpm must be positive",
        ));
    }
    if !(0.0..=1.0).contains(&pedal) {
        return Err(SimError::invalid_control("pedal must fall in [0, 1]"));
    }

    let dt = config.config().solver.fixed_step_s;
    let mut sim = Simulation::new(
        config.clone(),
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )?;
    // The dynamometer supplies the load by holding speed, so the external load
    // control stays at zero.
    sim.set_controls(Controls {
        pedal,
        load_torque_nm: 0.0,
        starter: false,
        ignition: true,
        egr_enabled,
    })?;

    let cycle_s = 2.0 * 60.0 / rpm;
    let steps_per_cycle = ((cycle_s / dt).round() as u32).max(PINS_PER_CYCLE);
    let pin_batch = (steps_per_cycle / PINS_PER_CYCLE)
        .max(1)
        .min(config.config().solver.max_steps_per_batch);

    for _ in 0..settle_cycles {
        advance_cycle_pinned(&mut sim, rpm, pin_batch, steps_per_cycle)?;
    }

    let mut brake_torque = 0.0;
    let mut indicated_torque = 0.0;
    let mut friction_torque = 0.0;
    let mut pumping_torque = 0.0;
    let mut power = 0.0;
    let mut imep = 0.0;
    let mut bmep = 0.0;
    let mut fuel_mg = 0.0;
    let mut bsfc = 0.0;
    let mut peak_pressure: f64 = 0.0;
    let mut peak_temperature: f64 = 0.0;
    let mut residual = 0.0;
    let mut brake_min = f64::INFINITY;
    let mut brake_max = f64::NEG_INFINITY;

    for _ in 0..measure_cycles {
        advance_cycle_pinned(&mut sim, rpm, pin_batch, steps_per_cycle)?;
        let averages = sim.cycle_averages();
        let snapshot = sim.snapshot();

        brake_torque += averages.brake_torque_nm;
        brake_min = brake_min.min(averages.brake_torque_nm);
        brake_max = brake_max.max(averages.brake_torque_nm);
        indicated_torque += averages.indicated_torque_nm;
        friction_torque += averages.friction_torque_nm;
        pumping_torque += averages.pumping_torque_nm;
        power += averages.brake_power_w;
        imep += averages.imep_pa;
        bmep += averages.bmep_pa;
        fuel_mg += averages.fuel_per_cycle_mg;
        bsfc += averages.bsfc_g_per_kwh;
        peak_pressure = peak_pressure.max(snapshot.peak_pressure_pa_cycle);
        peak_temperature = peak_temperature.max(snapshot.peak_gas_temperature_k);
        residual += snapshot.residual_fraction;
    }

    let n = f64::from(measure_cycles);
    let cylinders = config.config().geometry.cylinders as f64;
    let fuel_per_cylinder_mg = if cylinders > 0.0 {
        (fuel_mg / n) / cylinders
    } else {
        0.0
    };
    let mean_brake = brake_torque / n;
    let snapshot = sim.snapshot();
    let air_fuel_ratio = if fuel_per_cylinder_mg > 0.0 {
        sim.mean_trapped_air_kg() / (fuel_per_cylinder_mg * 1.0e-6)
    } else {
        f64::INFINITY
    };
    let spread = if mean_brake.abs() > 1.0 {
        (brake_max - brake_min).abs() / mean_brake.abs()
    } else {
        0.0
    };

    Ok(OperatingPoint {
        rpm,
        pedal,
        brake_torque_nm: mean_brake,
        indicated_torque_nm: indicated_torque / n,
        friction_torque_nm: friction_torque / n,
        pumping_torque_nm: pumping_torque / n,
        brake_power_w: power / n,
        imep_pa: imep / n,
        bmep_pa: bmep / n,
        fuel_mg_per_cycle: fuel_per_cylinder_mg,
        bsfc_g_per_kwh: bsfc / n,
        peak_pressure_pa: peak_pressure,
        peak_gas_temperature_k: peak_temperature,
        air_fuel_ratio,
        intake_pressure_pa: snapshot.intake_pressure_pa,
        exhaust_pressure_pa: snapshot.exhaust_pressure_pa,
        turbo_shaft_rad_per_s: snapshot.turbo_shaft_rad_per_s,
        wastegate_position: snapshot.wastegate_position,
        egr_rate: snapshot.egr_rate,
        residual_fraction: residual / n,
        converged: spread <= CONVERGENCE_TOLERANCE,
    })
}

/// Advance one four-stroke cycle, re-pinning the crank speed as it goes.
fn advance_cycle_pinned(
    sim: &mut Simulation,
    rpm: f64,
    pin_batch: u32,
    steps_per_cycle: u32,
) -> Result<()> {
    let mut remaining = steps_per_cycle;
    while remaining > 0 {
        let steps = pin_batch.min(remaining);
        sim.advance(steps)?;
        sim.pin_speed_rpm(rpm)?;
        remaining -= steps;
    }
    Ok(())
}

/// Measure a full-load (or part-load) speed sweep.
pub fn sweep(config: &ValidatedConfig, options: SweepOptions) -> Result<Vec<OperatingPoint>> {
    options.validate()?;
    let speeds = options.speeds();
    let mut points = Vec::with_capacity(speeds.len());
    for rpm in speeds {
        points.push(operating_point(
            config,
            rpm,
            options.pedal,
            options.settle_cycles,
            options.measure_cycles,
            options.egr_enabled,
        )?);
    }
    Ok(points)
}

/// The peak power and peak torque of a measured sweep.
///
/// The engine speeds returned here are an outcome of *our* calibration. The
/// source manual does not publish the speeds at which the real engine reaches
/// its rated figures, and these must never be presented as OEM data.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepPeaks {
    pub peak_power_w: f64,
    pub peak_power_rpm: f64,
    pub peak_torque_nm: f64,
    pub peak_torque_rpm: f64,
    pub max_peak_pressure_pa: f64,
    pub best_bsfc_g_per_kwh: f64,
}

/// Extract the peaks from a sweep. Returns `None` for an empty sweep.
pub fn peaks(points: &[OperatingPoint]) -> Option<SweepPeaks> {
    let power = points
        .iter()
        .max_by(|a, b| a.brake_power_w.total_cmp(&b.brake_power_w))?;
    let torque = points
        .iter()
        .max_by(|a, b| a.brake_torque_nm.total_cmp(&b.brake_torque_nm))?;
    let max_pressure = points
        .iter()
        .map(|p| p.peak_pressure_pa)
        .fold(0.0_f64, f64::max);
    let best_bsfc = points
        .iter()
        .map(|p| p.bsfc_g_per_kwh)
        .filter(|b| *b > 0.0)
        .fold(f64::INFINITY, f64::min);

    Some(SweepPeaks {
        peak_power_w: power.brake_power_w,
        peak_power_rpm: power.rpm,
        peak_torque_nm: torque.brake_torque_nm,
        peak_torque_rpm: torque.rpm,
        max_peak_pressure_pa: max_pressure,
        best_bsfc_g_per_kwh: if best_bsfc.is_finite() {
            best_bsfc
        } else {
            0.0
        },
    })
}

/// Engine speed in rad/s, re-exported for callers building sweep bounds.
pub fn rpm_to_omega(rpm: f64) -> f64 {
    rpm_to_rad_per_s(rpm)
}
