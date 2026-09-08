//! Calibration probe: the starter as a machine, and the speed it cranks at.
//!
//! `cargo run --release -p sim-core --example starter_sweep`
//!
//! This is the working tool behind the starter calibration, and it exists for
//! the same reason `brake_sweep` does: the machine has one published number to
//! hit and several calibrated values to hit it with, so the fit has to be
//! *measured* rather than asserted.
//!
//! Two anchors, and they are different in kind:
//!
//! - **Maximum mechanical output against the published `rated_power_w`.** The
//!   solver does not read that figure anywhere — the circuit produces whatever
//!   power the calibrated resistance, torque constant and saturation knee give
//!   it, and this prints the error. The engine brake's published anchors are
//!   handled the same way and for the same reason: a model permitted to read its
//!   own answer proves nothing.
//! - **Cranking speed against heavy-duty practice**, which is 150 to 250 rpm and
//!   is *not* published by anybody. It is reported as an outcome, because it is
//!   one: it is where the starter's torque-speed curve crosses the engine's own
//!   demand, and neither side of that crossing was chosen to produce it.
//!
//! The second is the honest one to watch. The old two-field starter cranked this
//! engine at about 270 rpm on 1500 N m, which is faster than a loaded 12.8 litre
//! six turns over on any real hardware.

use sim_core::sim::starter;
use sim_core::{Catalog, Controls, ResetOptions, Simulation};

/// Crank speeds the machine curve is printed at.
const CURVE_RPM: [f64; 12] = [
    0.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 175.0, 200.0, 250.0, 300.0, 350.0,
];

/// Long enough for the engagement, the first fired cycles and the governor to
/// have finished arguing. `start` itself is 1.2 s of cranking.
const CRANK_STEPS: u32 = 40_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::builtin()?;
    let config = catalog.active_config();
    let cfg = config.config();
    let st = &cfg.starter;
    let ratio = st.gear_ratio();

    println!(
        "starter: {:.0} V nominal, {:.1} kW published, {} pinion teeth on {} ring teeth \
         = {ratio:.2}:1",
        st.system_voltage_v,
        st.rated_power_w / 1000.0,
        st.pinion_teeth,
        st.ring_gear_teeth,
    );
    println!(
        "circuit: {:.4} ohm, {:.4} N m/A unsaturated, saturating at {:.0} A, \
         drag {:.5} N m/(rad/s), mesh {:.0}%\n",
        st.circuit_resistance_ohm,
        st.torque_constant_nm_per_a2,
        st.field_saturation_a,
        st.internal_drag_nm_per_rad_s,
        st.mesh_efficiency * 100.0,
    );

    // --- the machine on its own ---
    println!("the machine, against crank speed:");
    println!(
        "{:>6} {:>8} {:>8} {:>7} {:>10} {:>10} {:>9}",
        "rpm", "motor", "amps", "volts", "pinion Nm", "crank Nm", "shaft kW"
    );
    let mut best_power_w = 0.0;
    let mut best_at_rpm = 0.0;
    for rpm in CURVE_RPM {
        let omega_crank = rpm / 60.0 * std::f64::consts::TAU;
        let omega_motor = omega_crank * ratio;
        let amps = starter::sweep_current_a(st, omega_motor);
        let pinion_nm = starter::shaft_torque_nm(st, omega_motor);
        let power_w = starter::shaft_power_w(st, omega_motor);
        if power_w > best_power_w {
            best_power_w = power_w;
            best_at_rpm = rpm;
        }
        println!(
            "{rpm:6.0} {:8.0} {amps:8.0} {:7.1} {pinion_nm:10.1} {:10.1} {:9.2}",
            omega_motor * 60.0 / std::f64::consts::TAU,
            st.system_voltage_v - amps * st.circuit_resistance_ohm,
            pinion_nm * ratio * st.mesh_efficiency,
            power_w / 1000.0,
        );
    }

    // Peak power sits between the tabulated speeds, so find it on a fine grid
    // rather than reporting the best row above as if it were the maximum.
    let mut peak_w = 0.0;
    let mut peak_rpm = 0.0;
    for i in 0..=4_000 {
        let rpm = 400.0 * f64::from(i) / 4_000.0;
        let omega_motor = rpm / 60.0 * std::f64::consts::TAU * ratio;
        let power_w = starter::shaft_power_w(st, omega_motor);
        if power_w > peak_w {
            peak_w = power_w;
            peak_rpm = rpm;
        }
    }
    let _ = (best_power_w, best_at_rpm);

    let error = 100.0 * (peak_w - st.rated_power_w) / st.rated_power_w;
    println!(
        "\npeak mechanical output {:.2} kW at {peak_rpm:.0} crank rpm, against the \
         published {:.1} kW: {error:+.1}%",
        peak_w / 1000.0,
        st.rated_power_w / 1000.0,
    );
    println!(
        "stall: {:.0} A pulling {:.1} V, {:.0} N m at the crank",
        starter::sweep_current_a(st, 0.0),
        st.system_voltage_v - starter::sweep_current_a(st, 0.0) * st.circuit_resistance_ohm,
        starter::stall_crank_torque_nm(st),
    );

    // --- and against the engine it has to turn ---
    //
    // Ignition off, so this is the machine against friction, accessories and
    // pumping alone. With fuelling it stops being a cranking speed and becomes
    // a start.
    println!("\ncranking a cold engine, ignition off:");
    let mut sim = Simulation::new(
        config.clone(),
        ResetOptions {
            seed: 0,
            initial_rpm: 0.0,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )?;
    sim.set_controls(Controls {
        pedal: 0.0,
        starter: true,
        ignition: false,
        ..Controls::default()
    })?;

    let mut sink = vec![0.0f32; 1_000 * sim.audio_path_count()];
    let mut samples: Vec<f64> = Vec::new();
    for chunk in 0..(CRANK_STEPS / 1_000) {
        sim.advance(1_000)?;
        sim.drain_audio(&mut sink);
        // The last third, once the engagement and the first turns are behind it.
        if chunk >= (CRANK_STEPS / 1_000) * 2 / 3 {
            samples.push(sim.rpm());
        }
    }
    let mean = samples.iter().sum::<f64>() / samples.len().max(1) as f64;
    let low = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let high = samples.iter().copied().fold(0.0f64, f64::max);
    let out = sim.starter();
    println!("  settled at {mean:.0} rpm (swinging {low:.0} to {high:.0} over the firing cycle)");
    println!(
        "  drawing {:.0} A at {:.1} V, delivering {:.0} N m to the crank",
        out.current_a, out.terminal_voltage_v, out.crank_torque_nm,
    );
    println!(
        "  mesh frequency {:.0} Hz at that speed ({} ring teeth)",
        mean / 60.0 * f64::from(st.ring_gear_teeth),
        st.ring_gear_teeth,
    );
    let verdict = if (150.0..=250.0).contains(&mean) {
        "inside the 150-250 rpm heavy-duty range"
    } else if mean < 150.0 {
        "BELOW the 150-250 rpm heavy-duty range: too slow"
    } else {
        "ABOVE the 150-250 rpm heavy-duty range: too fast"
    };
    println!("  {verdict}");

    // --- and does it actually start ---
    println!("\nstarting from cold, ignition on, no pedal:");
    let mut sim = Simulation::new(
        config.clone(),
        ResetOptions {
            seed: 0,
            initial_rpm: 0.0,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )?;
    sim.set_controls(Controls {
        pedal: 0.0,
        starter: true,
        ignition: true,
        ..Controls::default()
    })?;
    let dt = cfg.solver.fixed_step_s;
    let mut caught_at_s = None;
    for chunk in 0..200u32 {
        sim.advance(1_000)?;
        sim.drain_audio(&mut sink);
        let t = f64::from(chunk + 1) * 1_000.0 * dt;
        if caught_at_s.is_none() && !sim.starter().engagement.gt(&0.0) && sim.rpm() > 400.0 {
            caught_at_s = Some(t);
        }
        if chunk % 20 == 19 {
            let s = sim.starter();
            println!(
                "  {t:4.2} s: {:6.0} rpm  starter {:6.0} N m  {:5.0} A  {:4.1} V  \
                 engagement {:.2}  state {:?}",
                sim.rpm(),
                s.crank_torque_nm,
                s.current_a,
                s.terminal_voltage_v,
                s.engagement,
                sim.run_state(),
            );
        }
    }
    match caught_at_s {
        Some(t) => println!("  caught and the relay released at {t:.2} s"),
        None => println!("  DID NOT CATCH within {:.1} s", 200.0 * 1_000.0 * dt),
    }
    println!(
        "  settled at {:.0} rpm, governed idle target {:.0}",
        sim.rpm(),
        cfg.governor.idle_target_rpm
    );

    Ok(())
}
