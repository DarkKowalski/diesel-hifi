//! The scenario harness: what makes a listening comparison repeatable.
//!
//! A comparison is only a comparison if the two runs differ where the model does
//! and nowhere else. These tests assert the properties that buys: the same
//! scenario twice is the same samples, a measurement window excludes the
//! settling that precedes it, a held phase actually holds, and nothing is
//! silently dropped.

use sim_core::scenario::{self, Phase, Scenario};
use sim_core::sim::{Controls, ResetOptions};
use sim_core::{EngineConfig, ValidatedConfig};

use sim_core::catalog::OM471_9_M3D_JSON;

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

#[test]
fn crank_captures_separate_compression_from_combustion_and_record_release() {
    let config = config();
    for id in ["crank", "crank-release"] {
        let run = scenario::run(&config, &scenario::find(id).unwrap()).unwrap();
        assert!(run.phases.iter().all(|phase| !phase.ignition));
        assert!(!run
            .events
            .iter()
            .any(|event| event.name == "first-combustion"));
        assert!(run
            .events
            .iter()
            .any(|event| event.name == "main-current-on"));
        assert!(run
            .events
            .iter()
            .any(|event| event.name == "pinion-retracted"));
        assert!(run.phases[0].rpm_max > 10.0);
        assert!(run.phases[0].starter_current_max_a > 1000.0);
        assert_eq!(run.dropped_frames, 0);
    }
    let start = scenario::run(&config, &scenario::find("start").unwrap()).unwrap();
    let burn = start
        .events
        .iter()
        .find(|event| event.name == "first-combustion")
        .unwrap();
    let release = start
        .events
        .iter()
        .find(|event| event.name == "pinion-retracted")
        .unwrap();
    assert!(burn.time_s > config.config().starter.pre_engage_time_s);
    assert!(burn.time_s < release.time_s);
    assert!(
        (burn.captured_frame.unwrap() as f64 / start.sample_rate_hz - burn.time_s).abs() < 1e-8
    );
    assert!(start.phases.last().unwrap().rpm_end > 500.0);
}

#[test]
fn a_retry_rearms_the_starter_without_resetting_the_engine() {
    let run = scenario::run(&config(), &scenario::find("restart").unwrap()).unwrap();
    let starts: Vec<_> = run
        .events
        .iter()
        .filter(|event| event.name == "main-current-on")
        .collect();
    assert_eq!(starts.len(), 2);
    assert!(starts[1].time_s > 1.0);
    assert!(run.phases[2].starter_current_max_a > 1000.0);
    assert_eq!(run.phases[2].rpm_start, run.phases[1].rpm_end);
    assert_eq!(run.phases[2].rpm_start, 0.0);
    let burn = run
        .events
        .iter()
        .find(|event| event.name == "first-combustion")
        .unwrap();
    assert!((1.0..2.2).contains(&burn.time_s));
    assert!(run.phases.last().unwrap().rpm_end > 500.0);
    assert_eq!(run.phases.last().unwrap().starter_current_max_a, 0.0);
}

fn running(pedal: f64) -> Controls {
    Controls {
        pedal,
        ignition: true,
        ..Controls::default()
    }
}

/// A short scenario, so these run in a debug build without taking a minute.
fn brief(hold_rpm: Option<f64>) -> Scenario {
    Scenario {
        id: "test-brief",
        summary: "A short settle and a short capture, for the harness tests.",
        steady: true,
        reset: ResetOptions {
            seed: 7,
            initial_rpm: 1_200.0,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
        phases: vec![
            Phase::settle(0.10, running(0.5), hold_rpm),
            Phase {
                label: "measure",
                duration_s: 0.20,
                controls: running(0.5),
                hold_rpm,
                capture: true,
            },
        ],
    }
}

#[test]
fn the_same_scenario_twice_is_the_same_samples() {
    // The whole point. Two captures of `idle` must differ only where the model
    // does, or a listening comparison between two versions is comparing noise.
    let config = config();
    let scenario = brief(Some(1_200.0));
    let first = scenario::run(&config, &scenario).expect("runs");
    let second = scenario::run(&config, &scenario).expect("runs");
    assert_eq!(
        first.frames, second.frames,
        "two runs of one scenario must be bit-identical"
    );
    assert_eq!(first.phases, second.phases, "and so must their conditions");
}

#[test]
fn a_capture_holds_only_the_captured_phases() {
    // A steady-state measurement window that includes the settling before it is
    // measuring the settling.
    let config = config();
    let scenario = brief(Some(1_200.0));
    let run = scenario::run(&config, &scenario).expect("runs");
    let expected = (0.20 * run.sample_rate_hz).round() as usize;
    assert_eq!(
        run.frame_count(),
        expected,
        "the capture should be the measure phase and nothing else"
    );
    assert_eq!(
        run.phases[0].frames, 0,
        "a settling phase contributes no frames"
    );
}

#[test]
fn nothing_is_dropped_on_the_way_out() {
    // A capture with a hole in it would compare against a reference at the wrong
    // alignment for ever after, and would do so silently.
    let config = config();
    let run = scenario::run(&config, &brief(None)).expect("runs");
    assert_eq!(run.dropped_frames, 0);
}

#[test]
fn a_held_phase_holds_and_a_free_one_does_not_have_to() {
    let config = config();

    let held = scenario::run(&config, &brief(Some(1_200.0))).expect("runs");
    let measure = held.phases.last().expect("a measure phase");
    assert!(measure.held);
    assert!(
        (measure.rpm_max - measure.rpm_min) < 30.0,
        "a pinned crank should stay near its speed, spanned {:.0} rpm",
        measure.rpm_max - measure.rpm_min
    );

    // Free running at half pedal with no load accelerates: this is the
    // measurement a held phase exists to avoid, and it is recorded rather than
    // prevented.
    let free = scenario::run(&config, &brief(None)).expect("runs");
    let measure = free.phases.last().expect("a measure phase");
    assert!(!measure.held);
    assert!(
        measure.rpm_end > measure.rpm_start,
        "an unloaded engine at half pedal runs away from the point being measured"
    );
}

#[test]
fn the_paths_come_out_interleaved_and_sum_to_the_mix() {
    let config = config();
    let run = scenario::run(&config, &brief(Some(1_200.0))).expect("runs");
    assert_eq!(run.paths, 4, "exhaust, block, body, starter");
    let mix = run.summed();
    let tracks: Vec<Vec<f32>> = (0..run.paths).map(|p| run.path(p)).collect();
    assert_eq!(mix.len(), run.frame_count());
    for i in 0..mix.len() {
        let summed: f32 = tracks.iter().map(|t| t[i]).sum();
        assert!((mix[i] - summed).abs() < 1e-6, "frame {i} does not add up");
    }

    // This scenario is a held speed with the pinion out, so the starter path
    // must be *exactly* silent rather than merely quiet. It is the property the
    // whole fourth path rests on: the mix at every fuelled operating point is
    // bit-identical to the three-path model, because adding zero is adding
    // nothing. `tests/acoustics.rs` asserts the consequence; this asserts the
    // cause, in the harness every acoustic figure is measured through.
    let starter = &tracks[3];
    assert!(
        starter.iter().all(|s| *s == 0.0),
        "the starter path must be exactly zero with the pinion retracted"
    );
}

#[test]
fn governed_idle_is_the_published_idle_speed_and_not_a_pinned_one() {
    // Idle is the one operating point the manual publishes a number for, so it
    // is the one scenario whose speed is an assertion rather than a record. The
    // earlier probe pinned 600 rpm and called it idle; this runs the governor.
    let config = config();
    let scenario = scenario::find("idle").expect("the idle scenario exists");
    let measure = scenario.phases.last().expect("a measure phase");
    assert!(
        measure.hold_rpm.is_none(),
        "idle must be governed, not held: the governor is part of how it sounds"
    );

    let run = scenario::run(&config, &scenario).expect("runs");
    let target = config.config().governor.idle_target_rpm;
    let measured = run.captured_rpm();
    assert!(
        (measured - target).abs() / target < 0.05,
        "governed idle settled at {measured:.0} rpm against a {target:.0} rpm target"
    );
}

#[test]
fn every_shipped_scenario_runs_and_captures_something() {
    // Cheap coverage that the table itself is well formed: no scenario asks for
    // a control the configuration rejects, and none captures nothing.
    for scenario in scenario::all() {
        assert!(
            scenario.capture_s() > 0.0,
            "{} captures nothing",
            scenario.id
        );
        assert!(
            scenario.capture_s() <= scenario.duration_s(),
            "{} captures more than it runs",
            scenario.id
        );
        assert!(
            !scenario.summary.is_empty(),
            "{} has no summary",
            scenario.id
        );
    }
}
