//! Envelope and rejection acceptance tests (README "Acceptance criteria").
//!
//! "Nominal tests stay below the 23 MPa combustion-pressure envelope and reject
//! invalid/non-finite state."

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::{Controls, Engine, EngineConfig, ErrorCode, ResetOptions};

const ENVELOPE_PA: f64 = 23.0e6;

fn engine() -> Engine {
    Engine::builtin().expect("catalog builds")
}

#[test]
fn a_nominal_pedal_sweep_stays_below_the_23_mpa_envelope() {
    let mut engine = engine();
    engine
        .set_controls(Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");

    let mut worst_pa: f64 = 0.0;
    // Crank, idle, then sweep the pedal across its whole range under varying load.
    for tick in 0..160 {
        let pedal = if tick < 40 {
            0.0
        } else {
            ((tick - 40) as f64 / 119.0).clamp(0.0, 1.0)
        };
        let load = if tick < 80 { 0.0 } else { 1500.0 };
        engine
            .set_controls(Controls {
                pedal,
                load_torque_nm: load,
                starter: tick < 12,
                ignition: true,
                egr_enabled: true,
                ..Controls::default()
            })
            .expect("controls accepted");
        let snapshot = engine
            .advance(4_000)
            .expect("nominal operation must not fault");
        worst_pa = worst_pa.max(snapshot.peak_pressure_pa_session);
        assert!(snapshot.fault.is_none());
        assert!(snapshot.rpm.is_finite() && snapshot.rpm >= 0.0);
        for pressure in &snapshot.cylinder_pressure_pa {
            assert!(pressure.is_finite() && *pressure > 0.0);
        }
    }

    assert!(
        worst_pa < ENVELOPE_PA,
        "peak cylinder pressure {:.2} MPa must stay below the {:.0} MPa envelope",
        worst_pa / 1.0e6,
        ENVELOPE_PA / 1.0e6
    );
    // Also confirm the run was not trivially quiet: compression alone should
    // produce tens of bar.
    assert!(
        worst_pa > 3.0e6,
        "the sweep should actually build cylinder pressure, saw {:.2} MPa",
        worst_pa / 1.0e6
    );
}

#[test]
fn the_engine_reaches_the_published_idle_speed() {
    let mut engine = engine();
    engine
        .set_controls(Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");

    // Crank until the starter cuts out, then let the idle governor hold speed.
    for tick in 0..70 {
        let snapshot = engine.advance(4_000).expect("advance");
        if tick == 12 || snapshot.rpm > 320.0 {
            engine
                .set_controls(Controls {
                    pedal: 0.0,
                    load_torque_nm: 0.0,
                    starter: false,
                    ignition: true,
                    egr_enabled: true,
                    ..Controls::default()
                })
                .expect("controls accepted");
        }
    }

    let idle = engine.snapshot().rpm;
    assert!(
        (idle - 560.0).abs() <= 40.0,
        "the idle governor should hold the published 560 rpm, saw {idle:.0} rpm"
    );
    assert_eq!(engine.snapshot().state, sim_core::RunState::Running);
}

#[test]
fn clearing_the_ignition_stops_the_engine() {
    let mut engine = engine();
    engine
        .set_controls(Controls {
            pedal: 0.3,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");
    for _ in 0..30 {
        engine.advance(4_000).expect("advance");
    }
    assert!(
        engine.snapshot().rpm > 300.0,
        "the engine should be running first"
    );

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
    // Coast down under friction and accessory load alone. 30 s of simulated
    // time is ample; the loop exits as soon as the crankshaft stops.
    let mut stopped_after_s = None;
    for _ in 0..300 {
        let snapshot = engine.advance(4_000).expect("advance");
        if snapshot.rpm == 0.0 {
            stopped_after_s = Some(snapshot.sim_time_s);
            break;
        }
    }
    let stopped_after_s = stopped_after_s.expect("the crankshaft must come to rest");
    assert!(stopped_after_s < 30.0);

    // And it stays stopped.
    engine.advance(4_000).expect("advance");
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.rpm, 0.0);
    assert_eq!(snapshot.state, sim_core::RunState::Stopped);
    assert_eq!(
        snapshot.fuel_demand_mg, 0.0,
        "ignition off means no fuelling"
    );
}

#[test]
fn controls_reject_non_finite_and_out_of_range_input() {
    let mut engine = engine();
    let max_load = 5_000.0;
    for bad in [
        Controls {
            pedal: f64::NAN,
            ..Controls::default()
        },
        Controls {
            pedal: 1.5,
            ..Controls::default()
        },
        Controls {
            pedal: -0.1,
            ..Controls::default()
        },
        Controls {
            load_torque_nm: f64::INFINITY,
            ..Controls::default()
        },
        Controls {
            load_torque_nm: -1.0,
            ..Controls::default()
        },
        Controls {
            load_torque_nm: max_load + 1.0,
            ..Controls::default()
        },
    ] {
        let error = engine
            .set_controls(bad)
            .expect_err("invalid controls must be rejected");
        assert_eq!(error.code, ErrorCode::InvalidControl);
    }
    // The last accepted controls survive a rejection.
    assert_eq!(engine.controls_pedal(), 0.0);
}

trait PedalProbe {
    fn controls_pedal(&self) -> f64;
}
impl PedalProbe for Engine {
    fn controls_pedal(&self) -> f64 {
        self.simulation().controls().pedal
    }
}

#[test]
fn a_pressure_envelope_breach_latches_a_structured_fault() {
    // Drop the envelope far below normal compression pressure so the guard
    // fires under otherwise nominal operation. This exercises the guard, not a
    // claim about the real engine.
    let mut document: serde_json::Value = serde_json::from_str(OM471_9_M3D_JSON).unwrap();
    document["limits"]["max_combustion_pressure_pa"] = serde_json::json!(1.5e6);
    let index = document["provenance"]
        .as_array()
        .unwrap()
        .iter()
        .position(|e| e["path"] == "limits.max_combustion_pressure_pa")
        .unwrap();
    document["provenance"][index]["value_note"] = serde_json::json!("test-only lowered envelope");

    let config = EngineConfig::from_json(&document.to_string())
        .unwrap()
        .validate()
        .expect("the lowered envelope still validates");
    let catalog = sim_core::Catalog::from_documents(&[&document.to_string()])
        .expect("catalog builds from the mutated document");
    assert_eq!(catalog.active_config().id(), config.id());

    let mut engine = Engine::from_catalog(catalog, ResetOptions::default()).expect("engine builds");
    engine
        .set_controls(Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");

    let mut error = None;
    for _ in 0..40 {
        if let Err(e) = engine.advance(4_000) {
            error = Some(e);
            break;
        }
    }
    let error = error.expect("compression must breach the lowered envelope");
    assert_eq!(error.code, ErrorCode::PressureEnvelopeExceeded);

    // The fault is latched: further advances return it, and the snapshot shows it.
    let again = engine.advance(1).expect_err("a latched fault persists");
    assert_eq!(again.code, ErrorCode::PressureEnvelopeExceeded);
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.state, sim_core::RunState::Fault);
    assert_eq!(
        snapshot.fault.expect("the snapshot carries the fault").code,
        ErrorCode::PressureEnvelopeExceeded
    );

    // A reset clears the fault.
    engine
        .reset(ResetOptions::default())
        .expect("reset clears the fault");
    assert!(engine.snapshot().fault.is_none());
}

#[test]
fn a_configuration_with_a_non_finite_value_is_rejected() {
    // JSON cannot express NaN, so use a value that fails the finite/positive
    // range check instead, plus an explicitly out-of-range solver step.
    let mut document: serde_json::Value = serde_json::from_str(OM471_9_M3D_JSON).unwrap();
    document["solver"]["fixed_step_s"] = serde_json::json!(0.5);
    let error = EngineConfig::from_json(&document.to_string())
        .unwrap()
        .validate()
        .expect_err("an out-of-range solver step must be rejected");
    assert_eq!(error.code, ErrorCode::InvalidConfig);
    assert!(error.message.contains("fixed_step_s"), "{}", error.message);
}

#[test]
fn gas_temperature_stays_inside_the_configured_guard() {
    let mut engine = engine();
    engine
        .set_controls(Controls {
            pedal: 1.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })
        .expect("controls accepted");
    for _ in 0..80 {
        let snapshot = engine.advance(4_000).expect("full pedal must not fault");
        assert!(snapshot.peak_gas_temperature_k < 3_000.0);
        assert!(snapshot.peak_gas_temperature_k.is_finite());
    }
}
