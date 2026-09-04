//! Proves the solver is configuration-driven rather than engine-specific.
//!
//! The fixture is a synthetic inline-four with entirely invented geometry. It is
//! never shipped in the catalog and makes no published or derived claim: every
//! one of its parameters is classified `calibrated`. If the solver had OM 471
//! numbers or ID checks baked into it, this test would not run.

use sim_core::config::ProvenanceStatus;
use sim_core::{Catalog, Controls, Engine, EngineConfig, ResetOptions, BUILTIN_ENGINE_ID};

const TEST_INLINE_FOUR: &str = include_str!("fixtures/test-inline-four.json");

#[test]
fn the_fixture_makes_no_oem_claim() {
    let config = EngineConfig::from_json(TEST_INLINE_FOUR).expect("fixture parses");
    assert!(config
        .provenance
        .iter()
        .all(|e| e.status == ProvenanceStatus::Calibrated));
    assert_ne!(config.identity.id, BUILTIN_ENGINE_ID);
    assert!(!config.identity.manufacturer_reference.contains("Mercedes"));
}

#[test]
fn a_different_engine_runs_through_the_same_solver() {
    let catalog = Catalog::from_documents(&[TEST_INLINE_FOUR]).expect("fixture validates");
    assert_eq!(catalog.active_id(), "synthetic-test-inline-four");

    let summary = &catalog.list()[0];
    assert_eq!(summary.cylinders, 4);
    assert!((summary.displacement_l - 5.132).abs() < 0.01);

    let mut engine = Engine::from_catalog(catalog, ResetOptions::default()).expect("engine builds");
    engine
        .set_controls(Controls {
            pedal: 0.5,
            load_torque_nm: 200.0,
            starter: true,
            ignition: true,
        })
        .expect("controls accepted");

    for _ in 0..40 {
        let snapshot = engine.advance(4_000).expect("the fixture must run cleanly");
        assert!(snapshot.fault.is_none());
        assert_eq!(snapshot.cylinder_pressure_pa.len(), 4);
        for pressure in &snapshot.cylinder_pressure_pa {
            assert!(pressure.is_finite() && *pressure > 0.0);
        }
        assert!(snapshot.rpm.is_finite());
    }

    let final_snapshot = engine.snapshot();
    assert!(
        final_snapshot.rpm > 100.0,
        "the four-cylinder fixture should be turning, saw {} rpm",
        final_snapshot.rpm
    );
    assert!(final_snapshot.peak_pressure_pa_session < 23.0e6);
}

#[test]
fn the_fixture_produces_a_different_result_from_the_om471() {
    fn run(document: &str) -> f64 {
        let catalog = Catalog::from_documents(&[document]).expect("document validates");
        let mut engine =
            Engine::from_catalog(catalog, ResetOptions::default()).expect("engine builds");
        engine
            .set_controls(Controls {
                pedal: 0.5,
                load_torque_nm: 0.0,
                starter: true,
                ignition: true,
            })
            .expect("controls accepted");
        let mut snapshot = engine.snapshot();
        for _ in 0..20 {
            snapshot = engine.advance(4_000).expect("advance");
        }
        snapshot.rpm
    }

    let four = run(TEST_INLINE_FOUR);
    let six = run(sim_core::catalog::OM471_9_M3D_JSON);
    assert!(
        (four - six).abs() > 1.0,
        "different configurations must produce different behaviour, got {four} and {six} rpm"
    );
}

#[test]
fn a_multi_entry_catalog_selects_between_configurations() {
    // The shipped catalog holds one diesel engine, but the selection API is the
    // same one a multi-configuration build would use.
    let mut catalog =
        Catalog::from_documents(&[sim_core::catalog::OM471_9_M3D_JSON, TEST_INLINE_FOUR])
            .expect("both documents validate");

    assert_eq!(catalog.list().len(), 2);
    assert_eq!(catalog.active_id(), BUILTIN_ENGINE_ID);
    assert!(catalog.contains("synthetic-test-inline-four"));

    catalog
        .select("synthetic-test-inline-four")
        .expect("select");
    assert_eq!(catalog.active_id(), "synthetic-test-inline-four");
    assert_eq!(catalog.active_config().config().geometry.cylinders, 4);

    catalog.select(BUILTIN_ENGINE_ID).expect("select back");
    assert_eq!(catalog.active_config().config().geometry.cylinders, 6);

    assert!(catalog.select("nope").is_err());
    assert_eq!(catalog.active_id(), BUILTIN_ENGINE_ID);
}
