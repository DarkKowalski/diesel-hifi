use sim_core::{
    catalog::OM471_9_M3D_JSON, Controls, EngineConfig, ErrorCode, ResetOptions, Simulation,
};

fn descending() -> Simulation {
    let cfg = EngineConfig::from_json(OM471_9_M3D_JSON)
        .unwrap()
        .validate()
        .unwrap();
    let mut sim = Simulation::new(
        cfg,
        ResetOptions {
            initial_rpm: 2400.0,
            ..ResetOptions::default()
        },
    )
    .unwrap();
    sim.set_controls(Controls {
        gear: 12,
        road_grade_percent: -15.0,
        ..Controls::default()
    })
    .unwrap();
    sim
}

#[test]
fn downhill_overspeed_pauses_after_a_complete_finite_step() {
    let mut sim = descending();
    let error = sim.advance(1000).unwrap_err();
    assert_eq!(error.code, ErrorCode::SpeedLimitExceeded);
    let stopped = sim.snapshot();
    assert!(stopped.rpm > 2400.0 && stopped.rpm < 2400.01);
    assert_eq!(stopped.steps_advanced, 1);
    assert_eq!(
        stopped.sim_time_s,
        sim.config().config().solver.fixed_step_s
    );
    assert!(stopped.crank_angle_rad > 0.0);
    assert!(stopped.cylinder_pressure_pa.iter().all(|p| p.is_finite()));
    assert!(stopped.torque_driveline_nm < 0.0);
    assert_eq!(stopped.fuel_demand_mg, 0.0);
    assert_eq!(sim.audio_available(), 1);
    sim.set_controls(sim.controls()).unwrap();
    assert_eq!(sim.advance(1000).unwrap_err(), error);
    assert_eq!(
        sim.snapshot(),
        stopped,
        "retrying unchanged conditions must not creep beyond the limit"
    );
    assert!(sim
        .set_controls(Controls {
            road_grade_percent: -16.0,
            ..sim.controls()
        })
        .is_err());
    assert_eq!(sim.snapshot(), stopped);
}

#[test]
fn changed_controls_can_recover_without_reset() {
    for neutral in [false, true] {
        let mut sim = descending();
        sim.advance(1000).unwrap_err();
        let paused = sim.snapshot();
        let controls = if neutral {
            Controls {
                gear: 0,
                ..sim.controls()
            }
        } else {
            Controls {
                road_grade_percent: 0.0,
                ..sim.controls()
            }
        };
        sim.set_controls(controls).unwrap();
        assert!(sim.snapshot().fault.is_none());
        assert_eq!(sim.snapshot().crank_angle_rad, paused.crank_angle_rad);
        assert_eq!(sim.snapshot().sim_time_s, paused.sim_time_s);
        sim.advance(4000).unwrap();
        let recovered = sim.snapshot();
        assert!(recovered.rpm < 2400.0);
        assert_eq!(recovered.steps_advanced, paused.steps_advanced + 4000);
        assert_eq!(sim.starter().current_a, 0.0);
    }
}

#[test]
fn a_level_road_recovers_a_motoring_engine_at_different_crank_phases() {
    for phase in 0..12 {
        let cfg = EngineConfig::from_json(OM471_9_M3D_JSON)
            .unwrap()
            .validate()
            .unwrap();
        let mut sim = Simulation::new(
            cfg,
            ResetOptions {
                initial_rpm: 2200.0,
                initial_crank_rad: phase as f64,
                ..ResetOptions::default()
            },
        )
        .unwrap();
        sim.set_controls(Controls {
            gear: 12,
            road_grade_percent: -15.0,
            ..Controls::default()
        })
        .unwrap();
        let mut pause = None;
        let mut audio = [0.0; 80000];
        for _ in 0..20 {
            let result = sim.advance(20_000);
            sim.drain_audio(&mut audio);
            if let Err(error) = result {
                pause = Some(error);
                break;
            }
        }
        assert_eq!(pause.unwrap().code, ErrorCode::SpeedLimitExceeded);
        let before = sim.snapshot();
        sim.set_controls(Controls {
            road_grade_percent: 0.0,
            ..sim.controls()
        })
        .unwrap();
        sim.advance(4000).unwrap();
        assert!(sim.rpm() < 2400.0, "phase {phase}");
        assert_eq!(sim.snapshot().steps_advanced, before.steps_advanced + 4000);
        assert!(sim.snapshot().sim_time_s > before.sim_time_s);
    }
}

#[test]
fn crossing_the_speed_limit_is_batch_invariant() {
    let mut batch = descending();
    let mut single = batch.clone();
    batch.advance(1000).unwrap_err();
    single.advance(1).unwrap_err();
    assert_eq!(batch.snapshot(), single.snapshot());
    let mut a = [0.0; 4000];
    let mut b = [0.0; 4000];
    assert_eq!(batch.drain_audio(&mut a), single.drain_audio(&mut b));
    assert_eq!(a, b);
}

#[test]
fn reset_cannot_bypass_the_configured_speed_envelope() {
    let mut sim = descending();
    let before = sim.snapshot();
    let error = sim
        .reset(ResetOptions {
            initial_rpm: 2400.1,
            ..ResetOptions::default()
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidControl);
    assert_eq!(sim.snapshot(), before);
}

#[test]
fn the_speed_ceiling_comes_from_configuration() {
    let mut cfg = EngineConfig::from_json(OM471_9_M3D_JSON).unwrap();
    cfg.limits.max_rpm = 2300.0;
    let mut sim = Simulation::new(
        cfg.validate().unwrap(),
        ResetOptions {
            initial_rpm: 2300.0,
            ..ResetOptions::default()
        },
    )
    .unwrap();
    sim.set_controls(Controls {
        gear: 12,
        road_grade_percent: -15.0,
        ..Controls::default()
    })
    .unwrap();
    let error = sim.advance(100).unwrap_err();
    assert_eq!(error.code, ErrorCode::SpeedLimitExceeded);
    assert!(error.message.contains("2300 rpm"));
}

#[test]
fn changing_controls_cannot_clear_a_pressure_fault() {
    let mut cfg = EngineConfig::from_json(OM471_9_M3D_JSON).unwrap();
    cfg.limits.max_combustion_pressure_pa = 200_000.0;
    let mut sim = Simulation::new(
        cfg.validate().unwrap(),
        ResetOptions {
            initial_rpm: 1200.0,
            ..ResetOptions::default()
        },
    )
    .unwrap();
    let error = sim.advance(10_000).unwrap_err();
    assert_eq!(error.code, ErrorCode::PressureEnvelopeExceeded);
    sim.set_controls(Controls {
        ignition: false,
        ..Controls::default()
    })
    .unwrap();
    let fault = sim.snapshot();
    assert_eq!(sim.advance(100).unwrap_err(), error);
    assert_eq!(sim.snapshot(), fault);
}
