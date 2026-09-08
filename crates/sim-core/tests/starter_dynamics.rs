use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::sim::starter::StarterState;
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation};

fn config() -> EngineConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON).unwrap()
}

#[test]
fn a_locked_engine_stalls_the_motor_and_terminal_voltage_excludes_motor_losses() {
    let cfg = config().starter;
    let mut state = StarterState::default();
    for _ in 0..40_000 {
        state.advance(&cfg, true, 0.0, 0.0, 0.0, 0.000025);
    }
    let out = state.advance(&cfg, true, 0.0, 0.0, 0.0, 0.000025);
    let stall_current = cfg.system_voltage_v / cfg.circuit_resistance_ohm;
    assert!((out.current_a - stall_current).abs() < 0.01);
    assert!(out.rotor_rad_per_s.abs() < 1e-6);
    assert!(
        (out.terminal_voltage_v
            - stall_current * (cfg.circuit_resistance_ohm - cfg.supply_resistance_ohm))
            .abs()
            < 1e-4
    );
    assert!(
        out.terminal_voltage_v > 0.0,
        "locked motor terminals are not back-EMF"
    );
}

#[test]
fn released_motor_coasts_independently_of_engine_speed_and_eventually_stops() {
    let cfg = config().starter;
    let dt = 0.000025;
    let mut state = StarterState::default();
    for step in 0..16_000 {
        state.advance(&cfg, true, 200.0, 20.94, step as f64 * dt * 20.94, dt);
    }
    for _ in 0..400 {
        state.advance(&cfg, false, 200.0, 20.94, 0.0, dt);
    }
    let mut fast_engine = state;
    let coast = state.advance(&cfg, false, 0.0, 0.0, 0.0, dt);
    let overrun = fast_engine.advance(&cfg, false, 2000.0, 209.4, 0.0, dt);
    assert_eq!(coast.engagement, 0.0);
    assert_eq!(coast.current_a, 0.0);
    assert_eq!(coast.crank_torque_nm, 0.0);
    assert_eq!(coast.mesh_forcing, 0.0);
    assert!(coast.rotor_rad_per_s > 100.0);
    assert_ne!(coast.rotation_forcing, 0.0);
    assert_eq!(coast.rotor_rad_per_s, overrun.rotor_rad_per_s);
    assert_eq!(coast.rotation_forcing, overrun.rotation_forcing);
    let mut previous = coast.rotor_rad_per_s;
    for _ in 0..80_000 {
        let out = state.advance(&cfg, false, 2000.0, 209.4, 0.0, dt);
        assert!(out.rotor_rad_per_s <= previous);
        previous = out.rotor_rad_per_s;
    }
    assert_eq!(previous, 0.0);
}

#[test]
fn starter_gears_turn_the_engine_and_generate_unfuelled_compression_sound() {
    let cfg = config().validate().unwrap();
    let mut turning = Simulation::new(cfg.clone(), ResetOptions::default()).unwrap();
    let mut resting = Simulation::new(cfg, ResetOptions::default()).unwrap();
    turning
        .set_controls(Controls {
            starter: true,
            ignition: false,
            ..Controls::default()
        })
        .unwrap();
    resting
        .set_controls(Controls {
            ignition: false,
            ..Controls::default()
        })
        .unwrap();
    let mut energy = [0.0; 4];
    let mut sink = [0.0; 400];
    for _ in 0..400 {
        turning.advance(100).unwrap();
        resting.advance(100).unwrap();
        let written = turning.drain_audio(&mut sink);
        for frame in sink[..written * 4].chunks_exact(4) {
            for (i, sample) in frame.iter().enumerate() {
                energy[i] += f64::from(*sample).powi(2);
            }
        }
        resting.drain_audio(&mut sink);
    }
    assert_eq!(resting.snapshot().rpm, 0.0);
    assert!(turning.snapshot().rpm > 100.0);
    assert!(turning.snapshot().peak_pressure_pa_session > 1_000_000.0);
    assert_eq!(turning.first_combustion_time_s(), None);
    assert!(turning.starter().crank_torque_nm > 0.0);
    assert!(
        energy.iter().all(|e| *e > 1e-8),
        "every path has a mechanical source: {energy:?}"
    );
}

#[test]
fn pressure_sound_calibration_changes_audio_without_changing_engine_physics() {
    let a = config();
    let mut b = a.clone();
    b.audio.body_pressure_rate_gain_s = 0.0;
    b.audio.body_pressure_weights.reverse();
    let mut with = Simulation::new(a.validate().unwrap(), ResetOptions::default()).unwrap();
    let mut without = Simulation::new(b.validate().unwrap(), ResetOptions::default()).unwrap();
    let controls = Controls {
        starter: true,
        ignition: false,
        ..Controls::default()
    };
    with.set_controls(controls).unwrap();
    without.set_controls(controls).unwrap();
    let mut sound_differs = false;
    let mut a_sink = [0.0; 400];
    let mut b_sink = [0.0; 400];
    for _ in 0..300 {
        with.advance(100).unwrap();
        without.advance(100).unwrap();
        let a = with.snapshot();
        let b = without.snapshot();
        assert_eq!(a.rpm, b.rpm);
        assert_eq!(a.crank_angle_rad, b.crank_angle_rad);
        assert_eq!(a.cylinder_pressure_pa, b.cylinder_pressure_pa);
        assert_eq!(a.torque_net_nm, b.torque_net_nm);
        assert_eq!(with.starter(), without.starter());
        with.drain_audio(&mut a_sink);
        without.drain_audio(&mut b_sink);
        sound_differs |= a_sink != b_sink;
    }
    assert!(sound_differs);
}

#[test]
fn unsafe_drive_parameters_and_mismatched_cylinder_weights_are_rejected() {
    for mutate in [
        |c: &mut EngineConfig| c.starter.supply_resistance_ohm = c.starter.circuit_resistance_ohm,
        |c: &mut EngineConfig| c.starter.rotor_inertia_kg_m2 = 0.0,
        |c: &mut EngineConfig| c.starter.drive_stiffness_nm_per_rad = f64::NAN,
        |c: &mut EngineConfig| c.starter.commutator_segments = 0,
        |c: &mut EngineConfig| {
            c.audio.body_pressure_weights.pop();
        },
        |c: &mut EngineConfig| c.audio.body_pressure_weights[0] = f64::NAN,
    ] {
        let mut cfg = config();
        mutate(&mut cfg);
        assert!(cfg.validate().is_err());
    }
}
