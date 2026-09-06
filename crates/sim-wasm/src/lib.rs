//! Thin `wasm-bindgen` adapter over [`sim_core`].
//!
//! This crate contains no physics and no validation of its own. It translates
//! between `JsValue` and `sim-core` types, and nothing more.
//!
//! The boundary is deliberately coarse (README "WASM and worker API"): set controls
//! once, then
//! advance a whole batch of fixed steps and return a single compact snapshot.
//! There is no per-step call into JavaScript. All simulation memory is owned by
//! the WASM module; JavaScript holds only the [`SimHandle`] pointer and receives
//! plain, structurally cloned objects.
//!
//! Errors are always plain objects `{ code, message }`, never panics.

use serde::Serialize;
use serde_wasm_bindgen::Serializer;
use wasm_bindgen::prelude::*;

use sim_core::dyno::{self, PointOptions, SweepOptions};
use sim_core::{Controls, Engine, ResetOptions, SimError, API_VERSION};

/// Version of the WASM API surface.
#[wasm_bindgen]
pub fn api_version() -> u32 {
    API_VERSION
}

/// Snapshot schema version understood by this build.
#[wasm_bindgen]
pub fn snapshot_version() -> u32 {
    sim_core::snapshot::SNAPSHOT_VERSION
}

/// Serialize with plain JS objects and real `Map`-free output so the worker can
/// `postMessage` results without further conversion.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| internal_error(&format!("failed to serialize result: {e}")))
}

fn from_js<T: serde::de::DeserializeOwned>(value: JsValue, what: &str) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| {
        error_object(
            sim_core::ErrorCode::InvalidControl.as_str(),
            &format!("malformed {what}: {e}"),
        )
    })
}

/// The shape every rejection takes on the JavaScript side.
#[derive(Serialize)]
struct ErrorPayload<'a> {
    code: &'a str,
    message: &'a str,
}

fn error_object(code: &str, message: &str) -> JsValue {
    let payload = ErrorPayload { code, message };
    let serializer = Serializer::new().serialize_maps_as_objects(true);
    payload
        .serialize(&serializer)
        .unwrap_or_else(|_| JsValue::from_str(message))
}

fn internal_error(message: &str) -> JsValue {
    error_object("INTERNAL_ERROR", message)
}

fn sim_error(error: SimError) -> JsValue {
    error_object(error.code.as_str(), &error.message)
}

/// The simulation handle held by the Web Worker.
#[wasm_bindgen]
pub struct SimHandle {
    engine: Engine,
}

#[wasm_bindgen]
impl SimHandle {
    /// Build a handle over the configurations compiled into this module.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<SimHandle, JsValue> {
        let engine = Engine::builtin().map_err(sim_error)?;
        Ok(Self { engine })
    }

    /// Summaries of every available configuration.
    #[wasm_bindgen(js_name = listConfigs)]
    pub fn list_configs(&self) -> Result<JsValue, JsValue> {
        to_js(&self.engine.list_configs())
    }

    /// Stable ID of the active configuration.
    #[wasm_bindgen(js_name = activeConfigId)]
    pub fn active_config_id(&self) -> String {
        self.engine.active_config_id().to_string()
    }

    /// Sources and per-parameter provenance for the active configuration.
    #[wasm_bindgen(js_name = configProvenance)]
    pub fn config_provenance(&self) -> Result<JsValue, JsValue> {
        to_js(&self.engine.provenance())
    }

    /// Select a configuration by stable ID. Performs a deterministic reset.
    #[wasm_bindgen(js_name = selectConfig)]
    pub fn select_config(&mut self, id: &str) -> Result<(), JsValue> {
        self.engine.select_config(id).map_err(sim_error)
    }

    /// Reset with an explicit seed and initial conditions.
    pub fn reset(&mut self, options: JsValue) -> Result<(), JsValue> {
        let options: ResetOptions = if options.is_undefined() || options.is_null() {
            ResetOptions::default()
        } else {
            from_js(options, "reset options")?
        };
        self.engine.reset(options).map_err(sim_error)
    }

    /// Submit control inputs.
    #[wasm_bindgen(js_name = setControls)]
    pub fn set_controls(&mut self, controls: JsValue) -> Result<(), JsValue> {
        let controls: Controls = from_js(controls, "controls")?;
        self.engine.set_controls(controls).map_err(sim_error)
    }

    /// Advance `steps` fixed steps and return one compact snapshot.
    pub fn advance(&mut self, steps: u32) -> Result<JsValue, JsValue> {
        let snapshot = self.engine.advance(steps).map_err(sim_error)?;
        to_js(&snapshot)
    }

    /// Current telemetry without advancing.
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        to_js(&self.engine.snapshot())
    }

    /// Fixed integration step of the active configuration, in seconds.
    #[wasm_bindgen(js_name = fixedStepSeconds)]
    pub fn fixed_step_seconds(&self) -> f64 {
        self.engine.fixed_step_s()
    }

    /// Largest batch permitted in one `advance` call.
    #[wasm_bindgen(js_name = maxStepsPerBatch)]
    pub fn max_steps_per_batch(&self) -> u32 {
        self.engine.max_steps_per_batch()
    }

    /// Sample rate of the exhaust audio, in hertz.
    ///
    /// One sample per solver step, so this is the reciprocal of the fixed step:
    /// 40 kHz at 25 us. Consumers resample from here to their device rate.
    #[wasm_bindgen(js_name = audioSampleRate)]
    pub fn audio_sample_rate(&self) -> f64 {
        self.engine.simulation().audio_sample_rate_hz()
    }

    /// Take the audio produced since the last drain.
    ///
    /// Deliberately separate from the snapshot: a batch carries thousands of
    /// samples, and the snapshot has to stay compact. The returned array is a
    /// fresh copy, so the caller may transfer it onward without touching WASM
    /// memory.
    #[wasm_bindgen(js_name = drainAudio)]
    pub fn drain_audio(&mut self) -> Vec<f32> {
        let available = self.engine.simulation().audio_available();
        let mut out = vec![0.0f32; available];
        let written = self.engine.simulation_mut().drain_audio(&mut out);
        out.truncate(written);
        out
    }

    /// Samples lost because the consumer fell behind, since the last reset.
    #[wasm_bindgen(js_name = audioDropped)]
    pub fn audio_dropped(&self) -> u32 {
        self.engine
            .simulation()
            .audio_dropped()
            .min(u64::from(u32::MAX)) as u32
    }

    /// Measure one steady-state dynamometer point on the active configuration.
    ///
    /// Runs on a separate simulation instance, so the live one keeps running
    /// untouched. The caller drives the sweep one point at a time so the worker
    /// can report progress and stay responsive.
    #[wasm_bindgen(js_name = operatingPoint)]
    pub fn operating_point(&self, rpm: f64, options: JsValue) -> Result<JsValue, JsValue> {
        let options: PointOptions = if options.is_undefined() || options.is_null() {
            PointOptions::default()
        } else {
            from_js(options, "operating point options")?
        };
        let point = self
            .engine
            .operating_point(rpm, &options)
            .map_err(sim_error)?;
        to_js(&point)
    }

    /// Measure a whole engine-brake speed sweep at one stage, in one call.
    ///
    /// The returned `brakePowerW` is negative at every point: the engine is
    /// absorbing power, not making it.
    #[wasm_bindgen(js_name = brakeSweep)]
    pub fn brake_sweep(&self, stage: u8, options: JsValue) -> Result<JsValue, JsValue> {
        let options: SweepOptions = if options.is_undefined() || options.is_null() {
            SweepOptions::default()
        } else {
            from_js(options, "sweep options")?
        };
        let points = self.engine.brake_sweep(stage, options).map_err(sim_error)?;
        to_js(&points)
    }

    /// Measure a whole speed sweep in one call.
    #[wasm_bindgen(js_name = sweep)]
    pub fn sweep(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let options: SweepOptions = if options.is_undefined() || options.is_null() {
            SweepOptions::default()
        } else {
            from_js(options, "sweep options")?
        };
        let points = self.engine.sweep(options).map_err(sim_error)?;
        to_js(&points)
    }

    /// The engine speeds a sweep would visit, so the UI can show progress.
    #[wasm_bindgen(js_name = sweepSpeeds)]
    pub fn sweep_speeds(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let options: SweepOptions = if options.is_undefined() || options.is_null() {
            SweepOptions::default()
        } else {
            from_js(options, "sweep options")?
        };
        to_js(&options.speeds())
    }

    /// Peak power and torque of a measured sweep.
    ///
    /// The speeds reported here are an outcome of this project's calibration.
    /// The source manual does not publish the speeds at which the real engine
    /// reaches its rated figures.
    #[wasm_bindgen(js_name = sweepPeaks)]
    pub fn sweep_peaks(points: JsValue) -> Result<JsValue, JsValue> {
        let points: Vec<sim_core::OperatingPoint> = from_js(points, "sweep points")?;
        match dyno::peaks(&points) {
            Some(peaks) => to_js(&peaks),
            None => Ok(JsValue::NULL),
        }
    }
}
