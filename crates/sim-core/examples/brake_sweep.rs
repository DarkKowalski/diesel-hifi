//! Calibration probe: print the engine-brake sweep at every stage.
//!
//! `cargo run --release -p sim-core --example brake_sweep`
//!
//! This is the working tool behind the Milestone 4 brake calibration. It reports
//! absorbed power against the two published anchors for the fitted variant.
//!
//! The manual does not say which brake stage its published figures describe.
//! Maximum brake power is stage III, and that is what the anchors are compared
//! against here — an interpretation, printed as such, not a published fact.

use sim_core::dyno::{self, SweepOptions};
use sim_core::Catalog;

/// Settle cycles per point. The brake changes the exhaust manifold pressure,
/// which changes the turbine, which changes the manifold again, so a braking
/// point takes as long to converge as a fuelled one.
const SETTLE: u32 = 100;
const MEASURE: u32 = 12;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::builtin()?;
    let config = catalog.active_config();
    let brake = &config.config().engine_brake;

    println!(
        "engine brake variant {} — published anchors {:.0} kW at {:.0} rpm, \
         {:.0} kW at {:.0} rpm",
        brake.variant,
        brake.anchor_low_power_w / 1000.0,
        brake.anchor_low_rpm,
        brake.anchor_high_power_w / 1000.0,
        brake.anchor_high_rpm,
    );
    println!(
        "stage I acts on {} of {} cylinders; brake operates above {:.0} rpm\n",
        brake.stage1_cylinder_count,
        config.config().geometry.cylinders,
        brake.min_speed_rpm,
    );

    let options = SweepOptions {
        start_rpm: 1000.0,
        end_rpm: 2300.0,
        step_rpm: 100.0,
        pedal: 0.0,
        settle_cycles: SETTLE,
        measure_cycles: MEASURE,
        egr_enabled: true,
    };

    println!(
        "{:>6} {:>10} {:>10} {:>10} {:>8} {:>7} {:>7} {:>6} {:>5}",
        "rpm", "I kW", "II kW", "III kW", "peak MPa", "p_exh", "boost", "krpm_t", "conv"
    );

    let mut stages = Vec::new();
    for stage in 1..=3u8 {
        stages.push(dyno::brake_sweep(config, stage, options)?);
    }

    for (index, rpm) in options.speeds().iter().enumerate() {
        let third = &stages[2][index];
        println!(
            "{:6.0} {:10.1} {:10.1} {:10.1} {:8.2} {:7.2} {:7.2} {:6.0} {:>5}",
            rpm,
            -stages[0][index].brake_power_w / 1000.0,
            -stages[1][index].brake_power_w / 1000.0,
            -third.brake_power_w / 1000.0,
            third.peak_pressure_pa / 1.0e6,
            third.exhaust_pressure_pa / 1.0e5,
            third.intake_pressure_pa / 1.0e5,
            third.turbo_shaft_rad_per_s * 60.0 / (2.0 * std::f64::consts::PI) / 1000.0,
            third.converged,
        );
    }

    println!("\n--- stage III against the published anchors ---");
    for (target_rpm, target_w) in [
        (brake.anchor_low_rpm, brake.anchor_low_power_w),
        (brake.anchor_high_rpm, brake.anchor_high_power_w),
    ] {
        let achieved = dyno::absorbed_power_w(config, target_rpm, 3, SETTLE, MEASURE)?;
        let error = (achieved - target_w) / target_w * 100.0;
        println!(
            "{:6.0} rpm: {:7.1} kW absorbed against {:5.0} kW published  ({:+.1}%)",
            target_rpm,
            achieved / 1000.0,
            target_w / 1000.0,
            error,
        );
    }

    // The brake must not have moved the fuelled calibration. Cheap to check here
    // and expensive to discover later.
    println!("\n--- brake off, fuelled, unchanged? ---");
    let fuelled = dyno::sweep(
        config,
        SweepOptions {
            start_rpm: 1000.0,
            end_rpm: 1800.0,
            step_rpm: 400.0,
            pedal: 1.0,
            settle_cycles: SETTLE,
            measure_cycles: MEASURE,
            egr_enabled: true,
        },
    )?;
    for point in &fuelled {
        println!(
            "{:6.0} rpm: {:7.1} Nm  {:6.1} kW",
            point.rpm,
            point.brake_torque_nm,
            point.brake_power_w / 1000.0
        );
    }

    Ok(())
}
