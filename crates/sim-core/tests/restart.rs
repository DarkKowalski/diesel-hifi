use sim_core::{catalog::OM471_9_M3D_JSON, Controls, EngineConfig, ResetOptions, Simulation};

fn config() -> EngineConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON).unwrap()
}

fn run(sim: &mut Simulation, seconds: f64, starter: bool, ignition: bool) {
    sim.set_controls(Controls {
        starter,
        ignition,
        ..Controls::default()
    })
    .unwrap();
    let steps = (seconds / sim.config().config().solver.fixed_step_s).round() as u32;
    advance(sim, steps);
}

fn advance(sim: &mut Simulation, mut steps: u32) {
    let mut audio = [0.0; 4000];
    while steps > 0 {
        let batch = steps.min(1000);
        sim.advance(batch).unwrap();
        sim.drain_audio(&mut audio);
        steps -= batch;
    }
}

#[test]
fn stopped_compression_relaxes_without_reset_or_starter_power() {
    for area in [0.0, config().air_path.ring_leakage_area_m2] {
        let mut cfg = config();
        cfg.air_path.ring_leakage_area_m2 = area;
        let mut sim = Simulation::new(cfg.validate().unwrap(), ResetOptions::default()).unwrap();
        run(&mut sim, 0.35, true, false);
        run(&mut sim, 0.65, false, false);
        let before = sim.snapshot();
        assert_eq!(before.rpm, 0.0);
        let cylinder = before
            .cylinder_pressure_pa
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
        let pressure = before.cylinder_pressure_pa[cylinder];
        assert!(
            pressure > 2e6,
            "compression must remain present, got {pressure}"
        );
        for _ in 0..10 {
            let old = sim.snapshot().cylinder_pressure_pa[cylinder];
            run(&mut sim, 0.05, false, false);
            let now = sim.snapshot();
            assert_eq!(now.rpm, 0.0);
            assert_eq!(now.crank_angle_rad, before.crank_angle_rad);
            assert_eq!(now.starter_current_a, 0.0);
            assert!(now.cylinder_pressure_pa[cylinder] <= old);
            assert!(
                now.cylinder_pressure_pa[cylinder] >= old * 0.98,
                "pressure must relax continuously, not be reset"
            );
        }
        let after = sim.snapshot().cylinder_pressure_pa[cylinder];
        if area == 0.0 {
            assert_eq!(after, pressure);
        } else {
            assert!(after < pressure * 0.95);
        }
        assert_eq!(sim.first_combustion_time_s(), None);
    }
}

#[test]
fn retries_at_different_stop_angles_and_waits_catch_and_remain_running() {
    let cfg = config().validate().unwrap();
    for crank in [0.15, 0.25, 0.35, 0.55, 0.85] {
        for pause in [0.1, 0.65, 2.0] {
            let mut sim = Simulation::new(cfg.clone(), ResetOptions::default()).unwrap();
            run(&mut sim, crank, true, false);
            run(&mut sim, pause, false, false);
            assert_eq!(sim.first_combustion_time_s(), None);
            run(&mut sim, 2.0, true, true);
            assert!(
                sim.first_combustion_time_s().is_some(),
                "no ignition: crank {crank}, pause {pause}"
            );
            run(&mut sim, 6.0, false, true);
            let rpm = sim.rpm();
            assert!(
                (500.0..620.0).contains(&rpm),
                "crank {crank}, pause {pause}: {rpm} rpm"
            );
            assert_eq!(sim.starter().current_a, 0.0);
            assert_eq!(sim.starter().crank_torque_nm, 0.0);
        }
    }
}

#[test]
fn a_perfectly_sealed_cylinder_reproduces_the_trapped_compression_stall() {
    let mut cfg = config();
    cfg.air_path.ring_leakage_area_m2 = 0.0;
    let mut sim = Simulation::new(cfg.validate().unwrap(), ResetOptions::default()).unwrap();
    run(&mut sim, 0.35, true, false);
    run(&mut sim, 0.65, false, false);
    run(&mut sim, 1.2, true, true);
    assert_eq!(sim.rpm(), 0.0);
    assert_eq!(sim.first_combustion_time_s(), None);
    assert!(sim.starter().current_a > 1800.0);
}

#[test]
fn a_running_engine_can_be_stopped_and_restarted_repeatedly() {
    let mut sim = Simulation::new(config().validate().unwrap(), ResetOptions::default()).unwrap();
    for _ in 0..3 {
        run(&mut sim, 2.0, true, true);
        run(&mut sim, 6.0, false, true);
        assert!((500.0..620.0).contains(&sim.rpm()));
        run(&mut sim, 3.0, false, false);
        assert_eq!(sim.rpm(), 0.0);
    }
}

#[test]
fn leakage_does_not_disguise_an_excessive_external_load() {
    let mut sim = Simulation::new(config().validate().unwrap(), ResetOptions::default()).unwrap();
    sim.set_controls(Controls {
        starter: true,
        ignition: true,
        load_torque_nm: 2000.0,
        ..Controls::default()
    })
    .unwrap();
    advance(&mut sim, 120_000);
    assert_eq!(sim.rpm(), 0.0);
    assert_eq!(sim.first_combustion_time_s(), None);
    assert!(sim.starter().current_a > 1800.0);
}

#[test]
fn invalid_ring_leakage_area_is_rejected() {
    for area in [-1e-9, 1.1e-6, f64::NAN, f64::INFINITY] {
        let mut cfg = config();
        cfg.air_path.ring_leakage_area_m2 = area;
        assert!(cfg.validate().is_err(), "accepted {area}");
    }
    let mut cfg = config();
    cfg.provenance
        .iter_mut()
        .find(|entry| entry.path == "air_path.ring_leakage_area_m2")
        .unwrap()
        .safe_range = Some([0.0, 1e-8]);
    assert!(
        cfg.validate().is_err(),
        "the declared calibration range must be enforced"
    );
}
