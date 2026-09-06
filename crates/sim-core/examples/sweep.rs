//! Calibration probe: print the full-load sweep and its peaks.
//!
//! `cargo run --release -p sim-core --example sweep`
//!
//! This is the working tool behind the Milestone 2 calibration. The engine
//! speeds it reports for peak power and peak torque are an outcome of that
//! calibration, not published OEM data.

use sim_core::dyno::{self, SweepOptions};
use sim_core::{Catalog, Controls, Engine, ResetOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::builtin()?;
    let config = catalog.active_config();

    println!("--- idle check ---");
    let mut engine = Engine::builtin()?;
    engine.set_controls(Controls {
        pedal: 0.0,
        load_torque_nm: 0.0,
        starter: true,
        ignition: true,
        egr_enabled: true,
        ..Controls::default()
    })?;
    for tick in 0..90 {
        let snapshot = engine.advance(4_000)?;
        if snapshot.rpm > 320.0 {
            engine.set_controls(Controls {
                pedal: 0.0,
                load_torque_nm: 0.0,
                starter: false,
                ignition: true,
                egr_enabled: true,
                ..Controls::default()
            })?;
        }
        if tick % 15 == 0 || tick == 89 {
            println!(
                "t={:5.1}s rpm={:7.1} peak={:6.2} MPa fuel={:6.2} mg boost={:5.2} bar \
                 delay={:4.2} deg beta={:4.2} brake={:7.1} Nm",
                snapshot.sim_time_s,
                snapshot.rpm,
                snapshot.peak_pressure_pa_cycle / 1.0e6,
                snapshot.fuel_per_cycle_mg,
                snapshot.intake_pressure_pa / 1.0e5,
                snapshot.ignition_delay_rad.to_degrees(),
                snapshot.premixed_fraction,
                snapshot.brake_torque_cycle_nm,
            );
        }
    }

    println!("\n--- full-load sweep ---");
    let options = SweepOptions {
        start_rpm: 600.0,
        end_rpm: 2000.0,
        step_rpm: 100.0,
        pedal: 1.0,
        settle_cycles: 100,
        measure_cycles: 12,
        egr_enabled: true,
    };
    // Measure point by point so a single envelope breach reports its speed
    // instead of aborting the whole sweep.
    let mut points = Vec::new();
    for rpm in options.speeds() {
        match dyno::operating_point(config, rpm, &options.point_options()) {
            Ok(point) => points.push(point),
            Err(error) => println!("{rpm:6.0} rpm FAILED: {error}"),
        }
    }

    println!(
        "{:>6} {:>9} {:>9} {:>8} {:>8} {:>8} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}",
        "rpm",
        "brake Nm",
        "power kW",
        "peak MPa",
        "fuel mg",
        "g/kWh",
        "AFR",
        "boost",
        "p_exh",
        "wgate",
        "EGR%",
        "krpm_t",
        "conv"
    );
    for p in &points {
        println!(
            "{:>6.0} {:>9.1} {:>9.1} {:>8.2} {:>8.1} {:>8.1} {:>7.1} {:>6.2} {:>6.2} {:>6.2} \
             {:>6.1} {:>6.0} {:>5}",
            p.rpm,
            p.brake_torque_nm,
            p.brake_power_w / 1000.0,
            p.peak_pressure_pa / 1.0e6,
            p.fuel_mg_per_cycle,
            p.bsfc_g_per_kwh,
            p.air_fuel_ratio,
            p.intake_pressure_pa / 1.0e5,
            p.exhaust_pressure_pa / 1.0e5,
            p.wastegate_position,
            p.egr_rate * 100.0,
            p.turbo_shaft_rad_per_s * 60.0 / (2.0 * std::f64::consts::PI) / 1000.0,
            p.converged,
        );
    }

    if let Some(peaks) = dyno::peaks(&points) {
        let power_error = (peaks.peak_power_w - 375_000.0) / 375_000.0 * 100.0;
        let torque_error = (peaks.peak_torque_nm - 2500.0) / 2500.0 * 100.0;
        println!(
            "\npeak power  {:7.1} kW at {:6.0} rpm   ({:+.1}% vs published 375 kW)",
            peaks.peak_power_w / 1000.0,
            peaks.peak_power_rpm,
            power_error
        );
        println!(
            "peak torque {:7.1} Nm at {:6.0} rpm   ({:+.1}% vs published 2500 Nm)",
            peaks.peak_torque_nm, peaks.peak_torque_rpm, torque_error
        );
        println!(
            "max peak cylinder pressure {:.2} MPa (envelope 23.00 MPa)",
            peaks.max_peak_pressure_pa / 1.0e6
        );
        println!("best BSFC {:.1} g/kWh", peaks.best_bsfc_g_per_kwh);
    }

    // Keep the unused-import warning away when the idle block is edited out.
    let _ = ResetOptions::default();
    Ok(())
}
