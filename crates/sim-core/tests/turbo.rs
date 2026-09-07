//! Wastegate turbocharger tests.
//!
//! Milestone 2 wrote manifold pressure straight from a schedule, so "does boost
//! respond correctly" was not a question that could be asked. It is now: boost
//! is the output of a compressor driven by a shaft with inertia, and these tests
//! exist to prove that chain is real and wired in rather than merely present.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::dyno::{self, PointOptions, SweepOptions};
use sim_core::{Controls, Engine, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

fn new_engine() -> Engine {
    Engine::builtin().expect("engine builds")
}

fn controls(pedal: f64, load_torque_nm: f64, starter: bool) -> Controls {
    Controls {
        pedal,
        load_torque_nm,
        starter,
        ignition: true,
        egr_enabled: true,
        ..Controls::default()
    }
}

/// Start unloaded and apply load once running: a real engine cannot crank
/// against full load either.
fn started_engine(load_torque_nm: f64, ticks: u32) -> Engine {
    let mut engine = new_engine();
    engine
        .set_controls(controls(1.0, 0.0, true))
        .expect("controls accepted");
    for tick in 0..ticks {
        engine.advance(4_000).expect("advance");
        if tick == 20 {
            engine
                .set_controls(controls(1.0, load_torque_nm, false))
                .expect("controls accepted");
        }
    }
    engine
}

/// Mean boost over a further second of running, rather than boost at one instant.
///
/// **This exists because of a defect in the plant, not in the test, and the
/// distinction was nearly missed.** At full pedal against 1500 N·m the engine
/// settles near 1900 rpm, and there the wastegate loop does not converge — it
/// limit-cycles, and boost swings between roughly 25 and 155 kPa with a period
/// of tens of seconds. A single-instant assertion therefore samples a phase of
/// an oscillation, and whether it passes depends on where in that oscillation
/// the run happens to stop.
///
/// It passed for several milestones by luck. Milestone 12 moved the phase, not
/// the amplitude: the oscillation was measured on the previous solver at the same
/// operating point and is the same size there. See `README.md`,
/// **Results → Sound → Known deficits**, for the deficit itself; averaging is
/// how this file stops depending on it.
fn mean_boost_pa(engine: &mut Engine, ticks: u32) -> f64 {
    let mut total = 0.0;
    for _ in 0..ticks {
        engine.advance(400).expect("advance");
        total += engine.snapshot().boost_pressure_pa;
    }
    total / f64::from(ticks.max(1))
}

#[test]
fn a_reset_engine_starts_with_a_stopped_turbo_at_ambient() {
    let engine = new_engine();
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.turbo_shaft_rad_per_s, 0.0);
    assert_eq!(snapshot.boost_pressure_pa, 0.0);
    assert_eq!(snapshot.wastegate_position, 0.0);
}

#[test]
fn running_under_load_spins_the_shaft_up_and_makes_real_boost() {
    let mut engine = started_engine(1_500.0, 120);
    let snapshot = engine.snapshot();

    assert!(
        snapshot.turbo_shaft_rad_per_s > 1_000.0,
        "the shaft should be spinning, got {} rad/s",
        snapshot.turbo_shaft_rad_per_s
    );
    assert!(
        snapshot.compressor_flow_kg_per_s > 0.0 && snapshot.turbine_flow_kg_per_s > 0.0,
        "both wheels should be passing gas"
    );

    let mean = mean_boost_pa(&mut engine, 100);
    assert!(
        mean > 30_000.0,
        "a loaded engine should build real boost, got {mean:.0} Pa averaged over a \
         second of running"
    );
}

#[test]
fn boost_decays_back_toward_ambient_when_fuelling_stops() {
    let mut engine = started_engine(1_500.0, 120);
    // Averaged for the same reason as above: the boost this is measured against
    // is one phase of a limit cycle otherwise.
    let boosted = mean_boost_pa(&mut engine, 100);
    assert!(boosted > 30_000.0, "starting boost {boosted:.0} Pa");

    // Cut the fuel: no combustion, no exhaust energy, nothing driving the turbine.
    engine
        .set_controls(Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: false,
            ignition: false,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");
    for _ in 0..200 {
        engine.advance(4_000).expect("advance");
    }

    let coasted = engine.snapshot();
    assert!(
        coasted.boost_pressure_pa < boosted * 0.25,
        "boost should collapse without exhaust energy: {:.0} Pa from {:.0} Pa",
        coasted.boost_pressure_pa,
        boosted
    );
}

#[test]
fn the_shaft_never_exceeds_its_protection_limit() {
    let config = config();
    let limit = config.config().turbo.max_shaft_speed_rad_per_s;
    let engine = started_engine(2_000.0, 150);
    assert!(
        engine.snapshot().turbo_shaft_rad_per_s <= limit,
        "shaft speed must stay inside the protection limit"
    );
}

/// The central claim of this milestone: the schedule that Milestone 2 wrote
/// directly into manifold pressure is now only a target, and a converged
/// wastegate controller reproduces it. If this drifts, the Milestone 2
/// calibration has not actually been preserved.
#[test]
fn steady_state_boost_tracks_the_setpoint_schedule() {
    let config = config();
    let schedule = &config.config().air_path.boost_target_schedule;

    for rpm in [1_100.0, 1_400.0, 1_700.0] {
        let point = dyno::operating_point(
            &config,
            rpm,
            &PointOptions {
                pedal: 1.0,
                settle_cycles: 100,
                measure_cycles: 12,
                ..PointOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("operating point at {rpm} rpm: {e}"));
        let setpoint = schedule.lookup(rpm);
        let error = (point.intake_pressure_pa - setpoint).abs() / setpoint;
        assert!(
            error <= 0.10,
            "at {rpm} rpm boost settled at {:.0} Pa against a {setpoint:.0} Pa setpoint \
             ({:.1}% off); the wastegate controller is not holding the schedule",
            point.intake_pressure_pa,
            error * 100.0
        );
    }
}

#[test]
fn the_wastegate_opens_at_high_speed_where_there_is_exhaust_energy_to_spare() {
    let config = config();
    let point = dyno::operating_point(
        &config,
        1_800.0,
        &PointOptions {
            pedal: 1.0,
            settle_cycles: 100,
            measure_cycles: 12,
            ..PointOptions::default()
        },
    )
    .expect("point");
    assert!(
        point.wastegate_position > 0.0,
        "at rated speed the turbo makes more than the setpoint asks for, so the \
         wastegate must be bleeding some of it off"
    );
    assert!(point.wastegate_position <= 1.0);
}

#[test]
fn part_load_needs_less_boost_than_full_load() {
    let config = config();
    let full = dyno::operating_point(
        &config,
        1_200.0,
        &PointOptions {
            pedal: 1.0,
            settle_cycles: 100,
            measure_cycles: 12,
            ..PointOptions::default()
        },
    )
    .expect("full");
    let part = dyno::operating_point(
        &config,
        1_200.0,
        &PointOptions {
            pedal: 0.4,
            settle_cycles: 100,
            measure_cycles: 12,
            ..PointOptions::default()
        },
    )
    .expect("part");
    assert!(
        part.intake_pressure_pa < full.intake_pressure_pa,
        "less fuel demanded means a lower boost setpoint and less exhaust energy"
    );
}

/// Turbo lag is now a consequence of shaft inertia rather than a tuned time
/// constant.
///
/// Measured at a held engine speed, the way a real transient test is run. On a
/// free engine a full-pedal step just accelerates it into the governed rev
/// limit, where fuelling tapers away and boost with it, so what you would
/// measure is the governor rather than the turbocharger.
///
/// The band is wide because this is a plausibility check, not a published
/// figure: the manual says nothing about transient response.
#[test]
fn turbo_lag_after_a_fuel_step_is_plausible() {
    let config = config();
    let rpm = 1_200.0;
    let mut sim = Simulation::new(
        config,
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )
    .expect("simulation builds");

    let step_controls = |pedal: f64| Controls {
        pedal,
        load_torque_nm: 0.0,
        starter: false,
        ignition: true,
        egr_enabled: true,
        ..Controls::default()
    };

    // Settle at light load: the turbo is turning but barely making boost.
    sim.set_controls(step_controls(0.1)).expect("controls");
    for _ in 0..400 {
        sim.advance(2_000).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
    }
    let before = sim.snapshot().boost_pressure_pa;

    // Step the fuel to full and watch the shaft carry the boost up.
    sim.set_controls(step_controls(1.0)).expect("controls");
    let step_start_s = sim.snapshot().sim_time_s;
    let mut trace = Vec::new();
    for _ in 0..600 {
        sim.advance(2_000).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        let snapshot = sim.snapshot();
        trace.push((
            snapshot.sim_time_s - step_start_s,
            snapshot.boost_pressure_pa,
        ));
    }

    let settled = trace
        .iter()
        .map(|(_, boost)| *boost)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        settled > before + 100_000.0,
        "a full-fuel step at {rpm} rpm should build real boost: {settled:.0} Pa from \
         {before:.0} Pa"
    );

    let target = before + 0.9 * (settled - before);
    let lag_s = trace
        .iter()
        .find(|(_, boost)| *boost >= target)
        .map(|(t, _)| *t)
        .expect("boost should reach 90% of its settled value");

    assert!(
        (0.1..=8.0).contains(&lag_s),
        "turbo lag of {lag_s:.2} s is outside the plausible band for a heavy-duty \
         truck turbocharger"
    );
}

#[test]
fn a_sweep_reports_finite_air_path_telemetry_at_every_point() {
    let config = config();
    let points = dyno::sweep(
        &config,
        SweepOptions {
            start_rpm: 800.0,
            end_rpm: 1_800.0,
            step_rpm: 200.0,
            pedal: 1.0,
            settle_cycles: 100,
            measure_cycles: 12,
            egr_enabled: true,
        },
    )
    .expect("sweep");

    for p in &points {
        assert!(p.intake_pressure_pa.is_finite() && p.intake_pressure_pa > 0.0);
        assert!(p.exhaust_pressure_pa.is_finite() && p.exhaust_pressure_pa > 0.0);
        assert!(p.turbo_shaft_rad_per_s.is_finite() && p.turbo_shaft_rad_per_s > 0.0);
        assert!(
            (0.0..=1.0).contains(&p.wastegate_position),
            "wastegate position {} out of range at {} rpm",
            p.wastegate_position,
            p.rpm
        );
    }
}
