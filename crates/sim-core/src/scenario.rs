//! Named, reproducible listening scenarios.
//!
//! A scenario is a seed, initial conditions and a script of control phases. It
//! exists so a comparison can be *regenerated* — the same idle, the same pull,
//! the same pedal release — rather than reproduced by hand on a slider and
//! described afterwards from memory. Two recordings of "roughly full load" are
//! not a comparison; two runs of `full-1400` are.
//!
//! This is a deterministic measurement harness in the same sense as [`crate::dyno`],
//! and it lives here for the same reason: it drives the simulation and touches
//! nothing outside it. Writing the result to a file is the caller's business and
//! happens in `examples/audio_capture.rs`.
//!
//! ## Held speed and free running are different measurements
//!
//! A phase either **holds** the crank at a stated speed, as a dynamometer does,
//! or lets it run free. Both are legitimate and they answer different questions,
//! so every scenario says which it is and every result reports the speed the
//! engine actually reached.
//!
//! Holding is how a steady spectrum is measured: an unloaded engine at full
//! pedal accelerates away from the point being measured within a few hundred
//! milliseconds, and a spectrum taken across that is smeared over whatever
//! range it swept. Holding is *not* a claim that a truck behaves this way. Free
//! running is how a transient is measured, and there the speed is an outcome.
//!
//! ## Idle is governed, not held
//!
//! The idle scenario runs the governor and lets the engine sit where it sits.
//! Pinning an engine to a chosen speed and calling the result idle measures a
//! held speed with an idle-ish pedal, which is a different signal: the governor
//! is what makes an idling diesel hunt slightly, and hunting is part of how idle
//! sounds. The scenario states 560 rpm as an *expectation* and the run reports
//! what the governor delivered.

use crate::error::Result;
use crate::sim::{Controls, ResetOptions, Simulation};
use crate::ValidatedConfig;

/// Steps between control updates and audio drains.
///
/// Fixed rather than tuned, because it is part of what makes a scenario
/// reproducible: a held phase re-pins the crank every chunk, so the chunk length
/// is one of the run's conditions. 100 steps is 2.5 ms.
pub const CHUNK_STEPS: u32 = 100;

/// One stretch of a scenario with the controls held constant.
#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    /// Short name, used in the manifest and in exported file names.
    pub label: &'static str,
    pub duration_s: f64,
    pub controls: Controls,
    /// Speed to re-pin the crank to each chunk, or `None` to run free.
    pub hold_rpm: Option<f64>,
    /// Whether this phase's audio is part of the capture.
    ///
    /// Settling phases are not captured. A steady-state measurement window that
    /// includes the start-up transient is measuring the start-up transient.
    pub capture: bool,
}

impl Phase {
    /// A settling phase: same controls, not captured.
    #[must_use]
    pub fn settle(duration_s: f64, controls: Controls, hold_rpm: Option<f64>) -> Self {
        Self {
            label: "settle",
            duration_s,
            controls,
            hold_rpm,
            capture: false,
        }
    }
}

/// A named run: what to reset to, and what to do next.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub id: &'static str,
    /// What the scenario is for, in one line.
    pub summary: &'static str,
    /// Whether the captured audio is a steady state or a transient.
    ///
    /// Steady captures may be averaged over and compared spectrum to spectrum.
    /// Transients may not: a spectrum of a run-down is a spectrum of every speed
    /// it passed through.
    pub steady: bool,
    pub reset: ResetOptions,
    pub phases: Vec<Phase>,
}

impl Scenario {
    /// Total scripted duration, captured and settling alike.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.phases.iter().map(|p| p.duration_s).sum()
    }

    /// Captured duration only.
    #[must_use]
    pub fn capture_s(&self) -> f64 {
        self.phases
            .iter()
            .filter(|p| p.capture)
            .map(|p| p.duration_s)
            .sum()
    }
}

/// What one phase actually did, as opposed to what it asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseResult {
    pub label: &'static str,
    pub captured: bool,
    pub held: bool,
    /// Frames captured in this phase; zero for a settling phase.
    pub frames: usize,
    pub pedal: f64,
    pub load_torque_nm: f64,
    pub brake_stage: u8,
    pub rpm_start: f64,
    pub rpm_end: f64,
    pub rpm_min: f64,
    pub rpm_max: f64,
    /// Mean over the phase, sampled once per chunk.
    pub rpm_mean: f64,
    /// Mean cycle brake torque over the phase, sampled once per chunk.
    pub brake_torque_nm: f64,
}

/// A completed scenario: its audio, and the conditions it ran under.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioRun {
    pub id: String,
    pub config_id: String,
    pub seed: u64,
    pub steady: bool,
    pub sample_rate_hz: f64,
    pub paths: usize,
    /// Interleaved frames from the captured phases, in order.
    pub frames: Vec<f32>,
    /// The limiter gain each captured frame carries: one entry per frame.
    ///
    /// Dividing it out of a frame recovers the mix the solver produced before
    /// the limiter engaged. It is carried here rather than recomputed because
    /// the gain has a release time, so it depends on the whole run rather than
    /// on the sample it was applied to.
    pub limiter_gains: Vec<f32>,
    pub phases: Vec<PhaseResult>,
    /// Frames the ring dropped because this harness fell behind.
    ///
    /// Must be zero. A capture with a hole in it is not a capture, and a
    /// silently shortened one would compare against a reference at the wrong
    /// alignment for ever after.
    pub dropped_frames: u64,
}

impl ScenarioRun {
    /// Number of captured frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len().checked_div(self.paths).unwrap_or(0)
    }

    /// One radiating path on its own.
    #[must_use]
    pub fn path(&self, index: usize) -> Vec<f32> {
        self.frames.chunks(self.paths).map(|f| f[index]).collect()
    }

    /// The three paths added up: what a listener hears before the cab stage.
    #[must_use]
    pub fn summed(&self) -> Vec<f32> {
        self.frames
            .chunks(self.paths)
            .map(|f| f.iter().sum())
            .collect()
    }

    /// Mean speed across the captured phases, weighted by their frames.
    #[must_use]
    pub fn captured_rpm(&self) -> f64 {
        let (sum, frames) = self
            .phases
            .iter()
            .filter(|p| p.captured)
            .fold((0.0, 0usize), |(sum, frames), p| {
                (sum + p.rpm_mean * p.frames as f64, frames + p.frames)
            });
        if frames == 0 {
            0.0
        } else {
            sum / frames as f64
        }
    }
}

/// Controls for a fuelled, EGR-enabled engine at a given pedal and load.
fn fuelled(pedal: f64, load_torque_nm: f64) -> Controls {
    Controls {
        pedal,
        load_torque_nm,
        starter: false,
        ignition: true,
        egr_enabled: true,
        brake_stage: 0,
        gear: 0,
        road_grade_percent: 0.0,
    }
}

/// Reset options for an engine already turning at `rpm`.
fn warm_start(seed: u64, rpm: f64) -> ResetOptions {
    ResetOptions {
        seed,
        initial_rpm: rpm,
        initial_crank_rad: 0.0,
        coolant_temp_k: 293.15,
    }
}

/// A steady point held at `rpm`, settled and then captured.
fn steady(
    id: &'static str,
    summary: &'static str,
    rpm: f64,
    pedal: f64,
    settle_s: f64,
    capture_s: f64,
) -> Scenario {
    let controls = fuelled(pedal, 0.0);
    Scenario {
        id,
        summary,
        steady: true,
        reset: warm_start(0, rpm),
        phases: vec![
            Phase::settle(settle_s, controls, Some(rpm)),
            Phase {
                label: "measure",
                duration_s: capture_s,
                controls,
                hold_rpm: Some(rpm),
                capture: true,
            },
        ],
    }
}

/// Every scenario the acoustic work is compared over.
///
/// Ordered from steady to transient. The steady points span the range the
/// engine is actually driven over rather than the three the first probe used,
/// because "sounds wrong" was never confined to one speed.
#[must_use]
pub fn all() -> Vec<Scenario> {
    let idle_controls = fuelled(0.0, 0.0);
    let brake_controls = Controls {
        brake_stage: 3,
        ..fuelled(0.0, 0.0)
    };
    // The two free-running transients are driven **in gear**, and that is not a
    // detail. The engine's own rotating inertia is 3.5 kg·m²; a full-pedal
    // release from idle against a resisting torque either stalls the engine or
    // slams it into the governor inside a tenth of a second, and neither is what
    // a truck accelerating sounds like. In gear the forty-tonne combination
    // appears at the crank as tens to hundreds of kilogram-metres-squared, and
    // the speed rises at a rate the ear can follow. The gear is a listening
    // choice and is recorded as one.
    let in_gear = |pedal: f64, gear: u32| Controls {
        pedal,
        gear,
        ..fuelled(pedal, 0.0)
    };
    let climbing = |pedal: f64, gear: u32| Controls {
        road_grade_percent: 3.0,
        ..in_gear(pedal, gear)
    };

    vec![
        // Governed idle. Free running on purpose: the governor is part of how
        // an idling diesel sounds, and pinning the crank removes it.
        Scenario {
            id: "idle",
            summary:
                "Governed idle, no pedal, no load. The engine sits where the governor puts it.",
            steady: true,
            reset: warm_start(0, 560.0),
            phases: vec![
                Phase::settle(4.0, idle_controls, None),
                Phase {
                    label: "measure",
                    duration_s: 1.2,
                    controls: idle_controls,
                    hold_rpm: None,
                    capture: true,
                },
            ],
        },
        steady(
            "light-900",
            "Light load at 900 rpm: long ignition delay, large premixed fraction, the rattle.",
            900.0,
            0.25,
            3.0,
            1.2,
        ),
        steady(
            "cruise-1200",
            "Steady 1200 rpm at 60% pedal, held.",
            1_200.0,
            0.60,
            3.0,
            1.2,
        ),
        steady(
            "full-1400",
            "Full pedal at 1400 rpm, held. Peak torque region.",
            1_400.0,
            1.00,
            3.0,
            1.2,
        ),
        steady(
            "rated-1800",
            "Full pedal at 1800 rpm, held. The calibrated power peak.",
            1_800.0,
            1.00,
            3.0,
            1.2,
        ),
        // Cranking and catching. Captured from the first step, because the
        // question is what starting sounds like.
        Scenario {
            id: "start",
            summary: "Starter cranking a stopped engine until it catches, then settling to idle.",
            steady: false,
            reset: ResetOptions {
                seed: 0,
                initial_rpm: 0.0,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
            phases: vec![
                // No pedal while cranking. The idle governor fuels it, which is
                // what actually happens: a driver turning a key is not also
                // holding a fifth of throttle, and a scenario that does flares
                // the engine to the governed ceiling the moment it catches.
                Phase {
                    label: "crank",
                    duration_s: 1.2,
                    controls: Controls {
                        starter: true,
                        ..idle_controls
                    },
                    hold_rpm: None,
                    capture: true,
                },
                Phase {
                    label: "catch",
                    duration_s: 2.8,
                    controls: idle_controls,
                    hold_rpm: None,
                    capture: true,
                },
            ],
        },
        // Shutting down. The run-down through every speed below idle is the
        // whole point, so this is explicitly not a steady capture.
        Scenario {
            id: "stop",
            summary: "Idling, then ignition off: the run-down to rest.",
            steady: false,
            reset: warm_start(0, 560.0),
            phases: vec![
                Phase::settle(3.0, idle_controls, None),
                Phase {
                    label: "rundown",
                    duration_s: 3.0,
                    controls: Controls {
                        ignition: false,
                        ..idle_controls
                    },
                    hold_rpm: None,
                    capture: true,
                },
            ],
        },
        // A loaded pull in gear, which is what an acceleration is. The engine
        // works against forty tonnes rather than against its own inertia, so the
        // speed climbs over seconds and the sound climbs with it.
        Scenario {
            id: "accelerate",
            summary:
                "Sixth gear at part pedal, then full pedal: a loaded pull up through the range.",
            steady: false,
            reset: warm_start(0, 900.0),
            phases: vec![
                Phase::settle(2.0, in_gear(0.35, 6), None),
                Phase {
                    label: "pull",
                    duration_s: 5.0,
                    controls: in_gear(1.0, 6),
                    hold_rpm: None,
                    capture: true,
                },
            ],
        },
        // Pedal release from a loaded state into overrun, also in gear: the
        // truck's inertia is what makes an overrun last long enough to hear.
        //
        // On a 3% climb, because on the flat forty tonnes in ninth barely slows
        // in three seconds: the speed would fall 35 rpm and the capture would be
        // of an engine that has stopped burning rather than of a truck losing
        // way. The grade is what makes the release audible as a release.
        Scenario {
            id: "release",
            summary:
                "Ninth gear pulling up a 3% climb, then the pedal released: the drop into overrun.",
            steady: false,
            reset: warm_start(0, 1_400.0),
            phases: vec![
                Phase::settle(2.5, climbing(0.8, 9), None),
                Phase {
                    label: "overrun",
                    duration_s: 3.0,
                    controls: climbing(0.0, 9),
                    hold_rpm: None,
                    capture: true,
                },
            ],
        },
        // The engine brake at its published anchor speed. Held, because a
        // descending truck holds a speed and because the anchors are stated at
        // one.
        Scenario {
            id: "brake-1300",
            summary: "Decompression brake, stage III, held at 1300 rpm with the pedal released.",
            steady: true,
            reset: warm_start(0, 1_300.0),
            phases: vec![
                Phase::settle(4.0, brake_controls, Some(1_300.0)),
                Phase {
                    label: "measure",
                    duration_s: 1.2,
                    controls: brake_controls,
                    hold_rpm: Some(1_300.0),
                    capture: true,
                },
            ],
        },
    ]
}

/// Look one up by ID.
#[must_use]
pub fn find(id: &str) -> Option<Scenario> {
    all().into_iter().find(|s| s.id == id)
}

/// Run a scenario and return its audio with the conditions it ran under.
///
/// Deterministic for a given configuration and scenario: the chunk length is
/// fixed, the pins happen on chunk boundaries, and `advance(n)` is defined to
/// equal `n` calls of `advance(1)`.
///
/// # Errors
///
/// Propagates any simulation fault, including a control the configuration
/// rejects and a breach of the pressure envelope.
pub fn run(config: &ValidatedConfig, scenario: &Scenario) -> Result<ScenarioRun> {
    let dt = config.config().solver.fixed_step_s;
    let mut sim = Simulation::new(config.clone(), scenario.reset)?;
    let paths = sim.audio_path_count();

    // Drained every chunk whether or not the phase is captured, so the ring
    // never overflows and a capture cannot acquire a hole.
    let mut sink = vec![0.0f32; CHUNK_STEPS as usize * paths];
    let mut gain_sink = vec![1.0f32; CHUNK_STEPS as usize];
    let mut frames: Vec<f32> = Vec::new();
    let mut limiter_gains: Vec<f32> = Vec::new();
    let mut results: Vec<PhaseResult> = Vec::new();

    for phase in &scenario.phases {
        sim.set_controls(phase.controls)?;
        if let Some(rpm) = phase.hold_rpm {
            sim.pin_speed_rpm(rpm)?;
        }

        let chunks = ((phase.duration_s / dt) / f64::from(CHUNK_STEPS)).round() as usize;
        let rpm_start = sim.snapshot().rpm;
        let mut rpm_min = rpm_start;
        let mut rpm_max = rpm_start;
        let mut rpm_sum = 0.0;
        let mut torque_sum = 0.0;
        let mut captured = 0usize;

        for _ in 0..chunks {
            sim.advance(CHUNK_STEPS)?;
            if let Some(rpm) = phase.hold_rpm {
                sim.pin_speed_rpm(rpm)?;
            }
            let written = sim.drain_audio_with_gains(&mut sink, &mut gain_sink);
            if phase.capture {
                frames.extend_from_slice(&sink[..written * paths]);
                limiter_gains.extend_from_slice(&gain_sink[..written]);
                captured += written;
            }

            let snapshot = sim.snapshot();
            rpm_min = rpm_min.min(snapshot.rpm);
            rpm_max = rpm_max.max(snapshot.rpm);
            rpm_sum += snapshot.rpm;
            torque_sum += snapshot.brake_torque_cycle_nm;
        }

        let divisor = chunks.max(1) as f64;
        results.push(PhaseResult {
            label: phase.label,
            captured: phase.capture,
            held: phase.hold_rpm.is_some(),
            frames: captured,
            pedal: phase.controls.pedal,
            load_torque_nm: phase.controls.load_torque_nm,
            brake_stage: phase.controls.brake_stage,
            rpm_start,
            rpm_end: sim.snapshot().rpm,
            rpm_min,
            rpm_max,
            rpm_mean: rpm_sum / divisor,
            brake_torque_nm: torque_sum / divisor,
        });
    }

    Ok(ScenarioRun {
        id: scenario.id.to_string(),
        config_id: config.config().identity.id.clone(),
        seed: scenario.reset.seed,
        steady: scenario.steady,
        sample_rate_hz: 1.0 / dt,
        paths,
        frames,
        limiter_gains,
        phases: results,
        dropped_frames: sim.audio_dropped(),
    })
}
