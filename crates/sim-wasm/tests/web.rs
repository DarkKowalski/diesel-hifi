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
    set("egrEnabled", JsValue::from_bool(true));
    set("brakeStage", JsValue::from_f64(0.0));
    set("gear", JsValue::from_f64(0.0));
    set("roadGradePercent", JsValue::from_f64(0.0));
    object.into()
}

/// Controls with the engine brake selected, in gear, on a descent.
fn braking_controls(stage: u32, gear: u32, grade_percent: f64) -> JsValue {
    let object = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&object, &JsValue::from_str(k), &v).expect("settable");
    };
    set("pedal", JsValue::from_f64(0.0));
    set("loadTorqueNm", JsValue::from_f64(0.0));
    set("starter", JsValue::from_bool(false));
    set("ignition", JsValue::from_bool(true));
    set("egrEnabled", JsValue::from_bool(true));
    set("brakeStage", JsValue::from_f64(f64::from(stage)));
    set("gear", JsValue::from_f64(f64::from(gear)));
    set("roadGradePercent", JsValue::from_f64(grade_percent));
    object.into()
}

/// Point options for one unfuelled engine-brake measurement.
fn braking_point_options(stage: u32, settle: u32, measure: u32) -> JsValue {
    let object = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&object, &JsValue::from_str(k), &v).expect("settable");
    };
    set("pedal", JsValue::from_f64(0.0));
    set("settleCycles", JsValue::from_f64(f64::from(settle)));
    set("measureCycles", JsValue::from_f64(f64::from(measure)));
    set("egrEnabled", JsValue::from_bool(true));
    set("ignition", JsValue::from_bool(false));
    set("brakeStage", JsValue::from_f64(f64::from(stage)));
    object.into()
}

#[wasm_bindgen_test]
fn exposes_stable_api_versions() {
    // The API version moves with new required calls; the snapshot protocol
    // stays at 2 because Milestones 3 and 4 only added members to it.
    assert_eq!(api_version(), 4);
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
    // At least one source, and every one carrying a title and a scope across the
    // boundary — rather than a fixed count. The engine's manual was the only
    // source until the starter arrived with a Bosch catalogue entry of its own,
    // and a count was never what this was checking.
    let sources = js_sys::Array::from(&get(&report, "sources"));
    assert!(sources.length() >= 1);
    for index in 0..sources.length() {
        let source = sources.get(index);
        assert!(!get(&source, "id").as_string().unwrap().is_empty());
        assert!(!get(&source, "title").as_string().unwrap().is_empty());
        assert!(!get(&source, "scope").as_string().unwrap().is_empty());
    }
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
    set("egrEnabled", JsValue::from_bool(true));
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
    // Boost must have built above ambient under load, and it is computed now
    // rather than prescribed, so the turbo state must be real too.
    assert!(number(&snapshot, "intakePressurePa") > 1.05e5);
    assert!(number(&snapshot, "boostPressurePa") > 0.0);
    assert!(number(&snapshot, "turboShaftRadPerS") > 0.0);
    assert!(number(&snapshot, "residualFraction") >= 0.0);
    assert!(number(&snapshot, "egrRate") >= 0.0);
    assert!(number(&snapshot, "wastegatePosition") >= 0.0);
}

#[wasm_bindgen_test]
fn audio_crosses_the_boundary_one_frame_per_step() {
    let mut handle = SimHandle::new().expect("handle constructs");

    // 25 us per step is a 40 kHz sample rate.
    assert!((handle.audio_sample_rate() - 40_000.0).abs() < 1.0e-6);

    // Four radiating paths interleaved into every frame: exhaust, block, body,
    // starter. They cross separately because they do not reach a listener by the
    // same route, and the browser gives each its own cab transfer.
    let paths = handle.audio_path_count() as usize;
    assert_eq!(paths, 4, "exhaust, block, body and starter");

    handle
        .set_controls(controls(0.5, 0.0, true, true))
        .expect("controls accepted");
    handle.advance(4_000).expect("advance");

    let samples = handle.drain_audio();
    assert_eq!(
        samples.len(),
        4_000 * paths,
        "one audio frame must be produced per solver step, {paths} floats to a frame"
    );
    for sample in &samples {
        assert!(
            sample.is_finite() && sample.abs() <= 1.0,
            "sample {sample} is outside the range an audio device accepts"
        );
    }

    // And the sum of a frame is what a listener hears, so it is the sum that
    // has to sit inside the soft-clip knee rather than each path separately.
    for frame in samples.chunks(paths) {
        let mixed: f32 = frame.iter().sum();
        assert!(
            mixed.is_finite() && mixed.abs() <= 1.0,
            "a frame summed to {mixed}, which no audio device accepts"
        );
    }

    // Draining is destructive: the space is freed for the next batch.
    assert_eq!(handle.drain_audio().len(), 0);
    assert_eq!(handle.audio_dropped(), 0);
}

#[wasm_bindgen_test]
fn a_running_engine_produces_audible_output() {
    let mut handle = SimHandle::new().expect("handle constructs");
    handle
        .set_controls(controls(0.6, 0.0, true, true))
        .expect("controls accepted");

    // Summed per frame, because what "audible" means is the sum: one loud path
    // cancelled by another is not a sound anyone hears.
    let paths = handle.audio_path_count() as usize;
    let mut loudest = 0.0f32;
    for tick in 0..60 {
        handle.advance(4_000).expect("advance");
        if tick == 20 {
            handle
                .set_controls(controls(0.6, 900.0, false, true))
                .expect("controls accepted");
        }
        for frame in handle.drain_audio().chunks(paths) {
            let mixed: f32 = frame.iter().sum();
            loudest = loudest.max(mixed.abs());
        }
    }
    assert!(
        loudest > 1.0e-3,
        "a running engine should not be silent, peak was {loudest}"
    );
}

#[wasm_bindgen_test]
fn a_dynamometer_point_crosses_the_boundary() {
    let handle = SimHandle::new().expect("handle constructs");
    let options = js_sys::Object::new();
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("settleCycles"),
        &JsValue::from_f64(30.0),
    )
    .expect("settable");
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("measureCycles"),
        &JsValue::from_f64(6.0),
    )
    .expect("settable");
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("pedal"),
        &JsValue::from_f64(1.0),
    )
    .expect("settable");
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("egrEnabled"),
        &JsValue::from_bool(true),
    )
    .expect("settable");
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("ignition"),
        &JsValue::from_bool(true),
    )
    .expect("settable");
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("brakeStage"),
        &JsValue::from_f64(0.0),
    )
    .expect("settable");
    let point = handle
        .operating_point(1200.0, options.into())
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

#[wasm_bindgen_test]
fn the_engine_brake_crosses_the_boundary_absorbing_power() {
    let handle = SimHandle::new().expect("handle constructs");
    let braked = handle
        .operating_point(1600.0, braking_point_options(3, 40, 8))
        .expect("brake point measures");

    // Absorbing, so the sign is negative. It stays negative rather than being
    // flipped for convenience: it is the same cycle-averaged work integral a
    // fuelled point reports.
    assert!(
        number(&braked, "brakePowerW") < 0.0,
        "a braking point must report negative power"
    );
    assert_eq!(number(&braked, "fuelMgPerCycle"), 0.0);
    assert_eq!(number(&braked, "brakeStage"), 3.0);

    let motored = handle
        .operating_point(1600.0, braking_point_options(0, 40, 8))
        .expect("motored point measures");
    assert!(
        number(&braked, "brakePowerW") < number(&motored, "brakePowerW"),
        "stage III must absorb more than plain motoring"
    );
}

#[wasm_bindgen_test]
fn the_brake_and_driveline_reach_the_snapshot() {
    let mut handle = SimHandle::new().expect("handle constructs");
    handle
        .set_controls(braking_controls(3, 9, -6.0))
        .expect("controls accepted");
    handle.reset(JsValue::UNDEFINED).expect("reset");
    handle
        .set_controls(braking_controls(3, 9, -6.0))
        .expect("controls accepted");

    // Reset leaves the crank stopped, so wind it up first: the brake is inert
    // below the published 1000 rpm floor.
    let mut snapshot = JsValue::UNDEFINED;
    for _ in 0..40 {
        snapshot = handle.advance(4_000).expect("advance");
    }

    for key in [
        "brakeStageActive",
        "brakeAbsorbedPowerW",
        "vehicleSpeedMPerS",
        "torqueDrivelineNm",
        "reflectedInertiaKgM2",
    ] {
        assert!(
            number(&snapshot, key).is_finite(),
            "`{key}` must arrive finite"
        );
    }
    assert_eq!(
        get(&snapshot, "gearEngaged"),
        JsValue::from_bool(true),
        "ninth gear must read as engaged"
    );
    assert!(
        number(&snapshot, "reflectedInertiaKgM2") > 1.0,
        "a laden truck in gear must reflect real inertia"
    );
    assert!(
        number(&snapshot, "vehicleSpeedMPerS") > 0.0,
        "a turning engine in gear must be moving"
    );
}

#[wasm_bindgen_test]
fn an_impossible_brake_stage_is_rejected_with_a_structured_error() {
    let mut handle = SimHandle::new().expect("handle constructs");
    let error = handle
        .set_controls(braking_controls(4, 0, 0.0))
        .expect_err("stage 4 does not exist");
    let message = get(&error, "message").as_string().unwrap_or_default();
    assert!(
        message.contains("brake_stage"),
        "the error must name the control, got `{message}`"
    );
}
