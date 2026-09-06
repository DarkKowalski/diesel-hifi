//! WASM smoke test for the `wasm-bindgen` boundary.
//!
//! Run with `pnpm wasm:test` (`wasm-pack test --node crates/sim-wasm`). This
//! exercises the compiled WebAssembly module and its JS-facing types without
//! needing a browser. The browser-and-worker path is covered separately by the
//! Playwright suite in `web/e2e`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use sim_wasm::{api_version, snapshot_version, SimHandle};

wasm_bindgen_test_configure!(run_in_node_experimental);

fn get(value: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(value, &JsValue::from_str(key)).expect("property is readable")
}

fn number(value: &JsValue, key: &str) -> f64 {
    get(value, key).as_f64().unwrap_or_else(|| {
        panic!("`{key}` should be a number");
    })
}

fn controls(pedal: f64, load: f64, starter: bool, ignition: bool) -> JsValue {
    let object = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&object, &JsValue::from_str(k), &v).expect("settable");
    };
    set("pedal", JsValue::from_f64(pedal));
    set("loadTorqueNm", JsValue::from_f64(load));
    set("starter", JsValue::from_bool(starter));
    set("ignition", JsValue::from_bool(ignition));
    object.into()
}

#[wasm_bindgen_test]
fn exposes_stable_api_versions() {
    assert_eq!(api_version(), 2);
    assert_eq!(snapshot_version(), 2);
}

#[wasm_bindgen_test]
fn lists_the_built_in_configuration() {
    let handle = SimHandle::new().expect("handle constructs");
    assert_eq!(handle.active_config_id(), "mercedes-benz-om471-9-m3d-375kw");

    let configs = handle.list_configs().expect("configs serialize");
    let array = js_sys::Array::from(&configs);
    assert_eq!(array.length(), 1);

    let summary = array.get(0);
    assert_eq!(
        get(&summary, "id").as_string().unwrap(),
        "mercedes-benz-om471-9-m3d-375kw"
    );
    assert_eq!(
        get(&summary, "displayName").as_string().unwrap(),
        "OM 471.9 M3D 375 kW Reference"
    );
    assert_eq!(number(&summary, "cylinders"), 6.0);
    assert_eq!(number(&summary, "ratedPowerKw"), 375.0);
    assert_eq!(number(&summary, "ratedTorqueNm"), 2500.0);
    assert_eq!(number(&summary, "idleRpm"), 560.0);
    assert!(!get(&summary, "disclaimer").as_string().unwrap().is_empty());
}

#[wasm_bindgen_test]
fn exposes_provenance_across_the_boundary() {
    let handle = SimHandle::new().expect("handle constructs");
    let report = handle.config_provenance().expect("provenance serializes");
    assert!(number(&report, "published_count") > 0.0);
    assert!(number(&report, "calibrated_count") > 0.0);
    let entries = js_sys::Array::from(&get(&report, "entries"));
    assert!(entries.length() > 40);
    let sources = js_sys::Array::from(&get(&report, "sources"));
    assert_eq!(sources.length(), 1);
}

#[wasm_bindgen_test]
fn rejects_an_unknown_configuration_id_with_a_structured_error() {
    let mut handle = SimHandle::new().expect("handle constructs");
    let error = handle
        .select_config("not-an-engine")
        .expect_err("unknown ids are rejected");
    assert_eq!(
        get(&error, "code").as_string().unwrap(),
        "UNKNOWN_CONFIG_ID"
    );
    assert!(get(&error, "message")
        .as_string()
        .unwrap()
        .contains("not-an-engine"));
    // The active selection is unchanged.
    assert_eq!(handle.active_config_id(), "mercedes-benz-om471-9-m3d-375kw");
}

#[wasm_bindgen_test]
fn rejects_out_of_range_controls_with_a_structured_error() {
    let mut handle = SimHandle::new().expect("handle constructs");
    let error = handle
        .set_controls(controls(2.0, 0.0, false, true))
        .expect_err("a pedal above 1 is rejected");
    assert_eq!(get(&error, "code").as_string().unwrap(), "INVALID_CONTROL");
}

#[wasm_bindgen_test]
fn resets_advances_and_returns_a_compact_snapshot() {
    let mut handle = SimHandle::new().expect("handle constructs");
    handle.reset(JsValue::UNDEFINED).expect("default reset");
    handle
        .set_controls(controls(0.5, 0.0, true, true))
        .expect("controls accepted");

    assert!(handle.fixed_step_seconds() > 0.0);
    assert!(handle.max_steps_per_batch() >= 1);

    let mut snapshot = handle.advance(4_000).expect("advance succeeds");
    for _ in 0..9 {
        snapshot = handle.advance(4_000).expect("advance succeeds");
    }

    assert_eq!(number(&snapshot, "schemaVersion"), 2.0);
    assert_eq!(
        get(&snapshot, "configId").as_string().unwrap(),
        "mercedes-benz-om471-9-m3d-375kw"
    );
    assert_eq!(number(&snapshot, "stepsAdvanced"), 40_000.0);
    assert!(number(&snapshot, "simTimeS") > 0.0);
    assert!(
        number(&snapshot, "rpm") > 0.0,
        "the engine should be turning after cranking"
    );

    let pressures = js_sys::Array::from(&get(&snapshot, "cylinderPressurePa"));
    assert_eq!(pressures.length(), 6);
    for index in 0..pressures.length() {
        let pressure = pressures.get(index).as_f64().expect("pressure is a number");
        assert!(pressure.is_finite() && pressure > 0.0);
    }
    assert!(number(&snapshot, "peakPressurePaSession") < 23.0e6);
    assert!(get(&snapshot, "fault").is_undefined() || get(&snapshot, "fault").is_null());
}

#[wasm_bindgen_test]
fn selecting_the_active_configuration_resets_deterministically() {
    let mut handle = SimHandle::new().expect("handle constructs");
    handle
        .set_controls(controls(0.5, 0.0, true, true))
        .expect("controls accepted");
    let running = handle.advance(8_000).expect("advance succeeds");
    assert!(number(&running, "rpm") > 0.0);

    handle
        .select_config("mercedes-benz-om471-9-m3d-375kw")
        .expect("re-selecting the active configuration succeeds");

    let after = handle.snapshot().expect("snapshot serializes");
    assert_eq!(number(&after, "rpm"), 0.0);
    assert_eq!(number(&after, "stepsAdvanced"), 0.0);
    assert_eq!(number(&after, "simTimeS"), 0.0);
}

#[wasm_bindgen_test]
fn rejects_a_batch_larger_than_the_configured_limit() {
    let mut handle = SimHandle::new().expect("handle constructs");
    let limit = handle.max_steps_per_batch();
    let error = handle
        .advance(limit + 1)
        .expect_err("an oversized batch is rejected");
    assert_eq!(
        get(&error, "code").as_string().unwrap(),
        "STEP_LIMIT_EXCEEDED"
    );
}

fn sweep_options(start: f64, end: f64, step: f64) -> JsValue {
    let object = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&object, &JsValue::from_str(k), &v).expect("settable");
    };
    set("startRpm", JsValue::from_f64(start));
    set("endRpm", JsValue::from_f64(end));
    set("stepRpm", JsValue::from_f64(step));
    set("pedal", JsValue::from_f64(1.0));
    set("settleCycles", JsValue::from_f64(30.0));
    set("measureCycles", JsValue::from_f64(6.0));
    object.into()
}

#[wasm_bindgen_test]
fn reports_the_cycle_averaged_torque_and_power() {
    let mut handle = SimHandle::new().expect("handle constructs");
    handle
        .set_controls(controls(0.6, 0.0, true, true))
        .expect("controls accepted");
    let mut snapshot = handle.advance(4_000).expect("advance");
    for tick in 0..60 {
        snapshot = handle.advance(4_000).expect("advance");
        // Load it once running. Unloaded, the engine runs into the governed
        // speed limit where fuelling — and therefore boost — falls back to zero.
        if tick == 20 {
            handle
                .set_controls(controls(0.6, 900.0, false, true))
                .expect("controls accepted");
        }
    }
    assert_eq!(get(&snapshot, "cycleValid").as_bool(), Some(true));
    assert!(number(&snapshot, "cyclesCompleted") > 1.0);
    assert!(number(&snapshot, "brakeTorqueCycleNm").is_finite());
    assert!(number(&snapshot, "bmepPa").is_finite());
    // Boost must have built above ambient under load.
    assert!(number(&snapshot, "intakePressurePa") > 1.05e5);
    assert!(number(&snapshot, "residualFraction") >= 0.0);
}

#[wasm_bindgen_test]
fn a_dynamometer_point_crosses_the_boundary() {
    let handle = SimHandle::new().expect("handle constructs");
    let point = handle
        .operating_point(1200.0, 1.0, 30, 6)
        .expect("operating point measures");
    assert!(number(&point, "brakeTorqueNm") > 500.0);
    assert!(number(&point, "brakePowerW") > 0.0);
    assert!(number(&point, "peakPressurePa") < 23.0e6);
    assert_eq!(number(&point, "rpm"), 1200.0);
}

#[wasm_bindgen_test]
fn a_sweep_crosses_the_boundary_with_its_peaks() {
    let handle = SimHandle::new().expect("handle constructs");
    let options = sweep_options(1000.0, 1400.0, 200.0);

    let speeds = js_sys::Array::from(&handle.sweep_speeds(options.clone()).expect("speeds"));
    assert_eq!(speeds.length(), 3);

    let points_value = handle.sweep(options).expect("sweep runs");
    let points = js_sys::Array::from(&points_value);
    assert_eq!(points.length(), 3);
    for index in 0..points.length() {
        let point = points.get(index);
        assert!(number(&point, "brakeTorqueNm").is_finite());
        assert!(number(&point, "bsfcGPerKwh") >= 0.0);
    }

    let peaks = SimHandle::sweep_peaks(points_value).expect("peaks");
    assert!(number(&peaks, "peakPowerW") > 0.0);
    assert!(number(&peaks, "peakTorqueNm") > 0.0);
    assert!(number(&peaks, "maxPeakPressurePa") < 23.0e6);
}

#[wasm_bindgen_test]
fn an_invalid_sweep_is_rejected_with_a_structured_error() {
    let handle = SimHandle::new().expect("handle constructs");
    let error = handle
        .sweep(sweep_options(2000.0, 1000.0, 100.0))
        .expect_err("a reversed sweep is rejected");
    assert_eq!(get(&error, "code").as_string().unwrap(), "INVALID_CONTROL");
}
