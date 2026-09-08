//! What the acoustic paths are actually being driven by, before any filtering.
//!
//! `cargo run --release -p sim-core --example trace_probe`
//!
//! `audio_probe` measures what leaves the boundary. This measures what goes in,
//! and it exists because the two questions are not the same one asked twice.
//! Every path filters: the structural path differentiates its forcing and rings
//! six resonators with the result, and the exhaust path sends its source down
//! metres of duct. Both spread a single defective step across tens of
//! milliseconds, so by the time a sample reaches the boundary a one-step
//! artefact and a genuine combustion event look alike — broadband, decaying,
//! at the firing rate.
//!
//! ## The question
//!
//! **Did this trace get where it is by being integrated, or by being assigned?**
//!
//! Anything a fixed-step solver integrates is continuous at the step, because a
//! derivative bounds how far it can move in 25 µs. Anything the solver *assigns*
//! is not: a phase boundary that clamps a cylinder to a manifold pressure, or a
//! trapped charge recomputed from a fresh gas-law evaluation, moves the trace by
//! whatever the two descriptions happen to disagree by, in one step.
//!
//! That distinction is invisible in the gas model, where it is a legitimate
//! modelling boundary and the error it makes is bounded and small. It stops
//! being invisible the moment the same trace is *radiated*, because the
//! structural path differentiates it and the derivative of a step is an impulse
//! and an impulse is white.
//!
//! So this is the check the acoustic plan puts before any source work: inspect
//! the traces for discontinuities and numerical artefacts *before* amplifying
//! their high-frequency content. Turning up a modal bank that is partly ringing
//! a boundary assignment buys more of the assignment.
//!
//! ## What it reports
//!
//! For each steady scenario, and for each of the three forcings:
//!
//! - how many samples the trace reached by assignment, and how large they are;
//! - which crank event each one coincides with, so a finding has a cause rather
//!   than an index;
//! - and the counterfactual — the same path run again from a trace with those
//!   steps interpolated away, so the contribution can be read as decibels and
//!   band shares rather than inferred.
//!
//! The counterfactual is a *measurement device*. Interpolating a solver's output
//! is a schedule bolted over a modelling boundary and does not belong in the
//! model; what belongs there is a boundary that does not step.

use sim_core::analysis::{
    crest_db, dbfs, firing_hz, jumps, rms, without_jumps, without_steps_at, Spectrum,
};
use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::scenario::{self, Scenario, CHUNK_STEPS};
use sim_core::sim::acoustics::{
    Acoustics, PATHS, PATH_BLOCK, PATH_BODY, PATH_EXHAUST, PATH_STARTER,
};
use sim_core::sim::cylinder::Phase;
use sim_core::sim::{AcousticForcing, Simulation};
use sim_core::{EngineConfig, ValidatedConfig};

/// How far a difference must stand above its neighbours to be called a step.
///
/// Not fitted. Measured on the shipped configuration the two populations are
/// nowhere near each other: a combustion pressure rise, which is the fastest
/// continuous thing the model produces, reads about 1.1, and the gas-exchange
/// transitions read 7 to 11. Three sits between them with a factor of two either
/// side, and `tests/analysis.rs` holds a pulse train against the same threshold
/// to show a fast continuous signal does not reach it.
const ISOLATION: f64 = 3.0;

/// Bands the acceptance criteria are written against, so the counterfactual is
/// reported in the same terms as everything else.
const BANDS: [(&str, f64, f64); 4] = [
    ("<80Hz", 0.0, 80.0),
    ("80-300", 80.0, 300.0),
    ("300-2k", 300.0, 2_000.0),
    (">2kHz", 2_000.0, f64::INFINITY),
];

/// The four forcings, recorded per step, with the crank state that produced
/// them.
struct Traces {
    rpm: f64,
    /// Volume velocity at the tailpipe mouth.
    mouth: Vec<f64>,
    /// Summed cylinder pressure over ambient.
    pressure: Vec<f64>,
    /// Gas plus pumping torque over rated torque.
    torque: Vec<f64>,
    /// Tooth-mesh force over the starter's stall torque.
    ///
    /// All zeros at every steady operating point, because the pinion is out at
    /// all of them. That is not a defect in the trace: it is the property that
    /// keeps the steady figures bit-identical, and it is why this one is
    /// reported from the transients instead.
    starter_mesh: Vec<f64>,
    /// Cylinder phases after each step, for attributing a step to its cause.
    phases: Vec<[Phase; 6]>,
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Intake => "intake",
        Phase::Closed => "closed",
        Phase::Exhaust => "exhaust",
    }
}

/// Replay a scenario one step at a time, recording the forcings.
///
/// The cadence matches `scenario::run` exactly — same controls, same pins, same
/// chunk length — and `advance(n)` is defined to equal `n` calls of
/// `advance(1)`, so these are the traces behind the very captures `audio_probe`
/// and `audio_capture` report. It is not a similar run at a similar speed.
fn replay(
    config: &ValidatedConfig,
    scenario: &Scenario,
) -> Result<Traces, Box<dyn std::error::Error>> {
    let dt = config.config().solver.fixed_step_s;
    let cylinders = config.config().geometry.cylinders;
    let mut sim = Simulation::new(config.clone(), scenario.reset)?;

    let mut traces = Traces {
        rpm: 0.0,
        mouth: Vec::new(),
        pressure: Vec::new(),
        torque: Vec::new(),
        starter_mesh: Vec::new(),
        phases: Vec::new(),
    };
    let mut sink = vec![0.0f32; CHUNK_STEPS as usize * sim.audio_path_count()];
    let mut rpm_sum = 0.0;
    let mut rpm_chunks = 0usize;

    for phase in &scenario.phases {
        sim.set_controls(phase.controls)?;
        if let Some(rpm) = phase.hold_rpm {
            sim.pin_speed_rpm(rpm)?;
        }
        let chunks = ((phase.duration_s / dt) / f64::from(CHUNK_STEPS)).round() as usize;
        for _ in 0..chunks {
            for _ in 0..CHUNK_STEPS {
                sim.advance(1)?;
                if phase.capture {
                    let forcing = sim.acoustic_forcing();
                    traces.mouth.push(forcing.mouth_volume_velocity);
                    traces.pressure.push(forcing.pressure_sum);
                    traces.torque.push(forcing.torque_fraction);
                    traces.starter_mesh.push(forcing.starter_mesh);
                    let mut here = [Phase::Closed; 6];
                    for (index, slot) in here.iter_mut().enumerate().take(cylinders) {
                        *slot = sim.cylinder_phase(index).unwrap_or(Phase::Closed);
                    }
                    traces.phases.push(here);
                }
            }
            if let Some(rpm) = phase.hold_rpm {
                sim.pin_speed_rpm(rpm)?;
            }
            // Drained and discarded: this probe measures the forcings, and a
            // ring left to overflow would report dropped frames that mean
            // nothing here.
            sim.drain_audio(&mut sink);
            if phase.capture {
                rpm_sum += sim.rpm();
                rpm_chunks += 1;
            }
        }
    }
    traces.rpm = if rpm_chunks > 0 {
        rpm_sum / rpm_chunks as f64
    } else {
        0.0
    };
    Ok(traces)
}

/// Whether the starter's mesh drive is continuous, and how loud it gets.
///
/// Reported for the transients because those are the only scenarios where the
/// pinion is ever in mesh. Two things can go wrong and both would be steps: the
/// pinion could arrive already loaded, or the relay could drop out with torque
/// still going through the mesh. Either is an edge in a radiated quantity, which
/// is the defect this probe was built for.
///
/// No firing-rate metric here, deliberately. A transient sweeps its own firing
/// rate and a comb share against the mean of a sweep measures a frequency the
/// engine held for a fraction of the capture.
fn report_starter_continuity(id: &str, mesh: &[f64]) {
    let peak = mesh.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if peak == 0.0 {
        return;
    }
    let largest_step = mesh
        .windows(2)
        .fold(0.0f64, |m, w| m.max((w[1] - w[0]).abs()));
    let found = jumps(mesh, ISOLATION);
    let engaged = mesh.iter().filter(|x| **x != 0.0).count();
    println!(
        "{id} — starter mesh drive: in mesh for {:>6} of {} samples, peak {peak:>6.3}, \
         largest single-step move {largest_step:>6.4}, blind steps {}",
        engaged,
        mesh.len(),
        found.len(),
    );
    println!(
        "              first contact and relay drop-out are both ramps, so the drive \
         enters and leaves at zero"
    );
    println!();
}

/// Run one path from a forcing trace, with the other gains zeroed.
///
/// The paths are driven through the real `Acoustics`, not through a copy of it,
/// so a counterfactual cannot drift away from the stage it is a counterfactual
/// about.
fn path_from(config: &ValidatedConfig, path: usize, trace: &[f64]) -> Vec<f32> {
    let cfg = config.config();
    let mut audio = cfg.audio.clone();
    audio.exhaust_gain = if path == PATH_EXHAUST {
        audio.exhaust_gain
    } else {
        0.0
    };
    audio.structural_gain = if path == PATH_BLOCK {
        audio.structural_gain
    } else {
        0.0
    };
    audio.body_gain = if path == PATH_BODY {
        audio.body_gain
    } else {
        0.0
    };
    audio.starter_gain = if path == PATH_STARTER {
        audio.starter_gain
    } else {
        0.0
    };

    let dt = cfg.solver.fixed_step_s;
    let mut acoustics = Acoustics::new(trace.len().max(1), &audio, dt);
    for sample in trace {
        // The trace drives exactly one path and the others are given nothing,
        // which is what makes the counterfactual below a statement about that
        // path rather than about the mix.
        let mut forcing = AcousticForcing::default();
        match path {
            PATH_EXHAUST => forcing.mouth_volume_velocity = *sample,
            PATH_BLOCK => forcing.pressure_sum = *sample,
            PATH_BODY => forcing.torque_fraction = *sample,
            _ => forcing.starter_mesh = *sample,
        }
        acoustics.push(&audio, &forcing, cfg.exhaust_system.radiation_cutoff_hz, dt);
    }
    let mut out = vec![0.0f32; trace.len() * PATHS];
    let written = acoustics.drain(&mut out);
    out[..written * PATHS]
        .chunks(PATHS)
        .map(|frame| frame[path])
        .collect()
}

/// Indices at which any cylinder changed phase: every valve event in the trace.
///
/// This is what the solver *knows*, as opposed to what a blind detector can
/// find, and the two are reported side by side deliberately. Isolation only sees
/// a step that stands above the slope it lands on; a gas-exchange step that
/// happens to arrive during a combustion rise is exactly as much of an
/// assignment and exactly as invisible.
fn valve_events(phases: &[[Phase; 6]]) -> Vec<usize> {
    (1..phases.len())
        .filter(|i| phases[*i] != phases[i - 1])
        .collect()
}

/// What removing a set of steps does to a path, in the terms the acceptance
/// criteria are written in.
fn counterfactual(
    config: &ValidatedConfig,
    label: &str,
    path: usize,
    trace: &[f64],
    repaired_trace: &[f64],
    f0: f64,
) {
    let rate = 1.0 / config.config().solver.fixed_step_s;
    let real = path_from(config, path, trace);
    let repaired = path_from(config, path, repaired_trace);
    // Skip the filters' onset: both runs start from a reset bank, and the
    // settling is not what is being compared.
    let skip = (real.len() / 8).min(8_192);
    let (real, repaired) = (&real[skip..], &repaired[skip..]);
    let (sr, ss) = (Spectrum::of(real, rate), Spectrum::of(repaired, rate));

    println!(
        "              {label:<22} {:>+6.2} dB level  {:>+6.2} dB crest  orders {:>+5.1} pts",
        dbfs(rms(repaired)) - dbfs(rms(real)),
        crest_db(repaired) - crest_db(real),
        ss.comb_share(f0, 4) - sr.comb_share(f0, 4),
    );
    let bands: Vec<String> = BANDS
        .iter()
        .map(|(name, lo, hi)| {
            format!(
                "{name} {:>5.1} -> {:>5.1}",
                sr.band_share(*lo, *hi),
                ss.band_share(*lo, *hi)
            )
        })
        .collect();
    println!("              {:22} {}", "", bands.join("  "));
}

/// Report one forcing: its steps, their causes, and what they are worth.
fn report_forcing(
    config: &ValidatedConfig,
    label: &str,
    path: usize,
    trace: &[f64],
    phases: &[[Phase; 6]],
    f0: f64,
) {
    // --- what a blind detector finds ---
    let found = jumps(trace, ISOLATION);
    let share = 100.0 * found.len() as f64 / trace.len().max(1) as f64;
    println!(
        "   {label:>9}: blind      {:>5} steps ({share:>5.2}%)  largest {:>9.4}  up to {:>6.1}x",
        found.len(),
        found.iter().fold(0.0f64, |m, j| m.max(j.size.abs())),
        found.iter().fold(0.0f64, |m, j| m.max(j.isolation)),
    );

    // Where the blind ones fell. A finding needs a cause: an index says a sample
    // is odd, a crank event says why.
    let events = valve_events(phases);
    let on_an_event = found
        .iter()
        .filter(|jump| events.binary_search(&jump.index).is_ok())
        .count();
    if !found.is_empty() {
        println!(
            "              of which {on_an_event} land on a valve event and {} do not",
            found.len() - on_an_event
        );
        counterfactual(
            config,
            "without the blind set:",
            path,
            trace,
            &without_jumps(trace, ISOLATION),
            f0,
        );
    }

    // --- and what the solver knows is there ---
    //
    // Every valve event, whether or not it stands out, grouped by the transition
    // that produced it and sized by how far the trace moved beyond what its
    // neighbours implied.
    let differences: Vec<f64> = trace.windows(2).map(|w| w[1] - w[0]).collect();
    let mut by_transition: std::collections::BTreeMap<(&str, &str), (usize, f64, f64)> =
        Default::default();
    for index in &events {
        let Some(k) = index.checked_sub(1) else {
            continue;
        };
        if k < 1 || k + 1 >= differences.len() {
            continue;
        }
        let excess = differences[k] - 0.5 * (differences[k - 1] + differences[k + 1]);
        let (before, after) = (&phases[index - 1], &phases[*index]);
        for cylinder in 0..before.len() {
            if before[cylinder] == after[cylinder] {
                continue;
            }
            let key = (phase_name(before[cylinder]), phase_name(after[cylinder]));
            let entry = by_transition.entry(key).or_insert((0, 0.0, 0.0));
            entry.0 += 1;
            entry.1 += excess.abs();
            entry.2 = entry.2.max(excess.abs());
        }
    }
    println!("              by cause   {:>5} valve events", events.len());
    for ((from, to), (count, total, peak)) in &by_transition {
        println!(
            "              {:>18} {count:>5}   mean excess {:>9.4}  peak {:>9.4}",
            format!("{from} -> {to}"),
            total / *count as f64,
            peak
        );
    }
    if !events.is_empty() {
        counterfactual(
            config,
            "without every event:",
            path,
            trace,
            &without_steps_at(trace, &events),
            f0,
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: ValidatedConfig = EngineConfig::from_json(OM471_9_M3D_JSON)?.validate()?;
    let cylinders = config.config().geometry.cylinders;

    println!(
        "what drives each path, before it is filtered\n\n\
         A step is a sample the solver assigned rather than integrated to: a lone\n\
         difference standing {ISOLATION}x above the differences either side of it. The\n\
         counterfactual line is the same path run again with those steps\n\
         interpolated away, which is a measurement and not a proposed fix.\n"
    );

    for scenario in scenario::all() {
        if !scenario.steady {
            // A transient trace sweeps its own firing rate, so the band shares
            // and order shares below would be averages over every speed it
            // passed through. `audio_capture` keeps those for listening.
            //
            // One thing is still worth asking here, and only here: the starter
            // is a transient by construction — it is in mesh for a second and
            // gone — so a steady scenario has nothing to say about it. The
            // question is the same one this whole probe asks, whether the trace
            // was integrated or assigned, and it is asked without any
            // firing-rate metric attached.
            let traces = replay(&config, &scenario)?;
            report_starter_continuity(scenario.id, &traces.starter_mesh);
            continue;
        }
        let traces = replay(&config, &scenario)?;
        let f0 = firing_hz(traces.rpm, cylinders);
        println!(
            "{} — {:.0} rpm, f0 {f0:.1} Hz, {} samples",
            scenario.id,
            traces.rpm,
            traces.pressure.len()
        );
        report_forcing(
            &config,
            "exhaust",
            PATH_EXHAUST,
            &traces.mouth,
            &traces.phases,
            f0,
        );
        report_forcing(
            &config,
            "pressure",
            PATH_BLOCK,
            &traces.pressure,
            &traces.phases,
            f0,
        );
        report_forcing(
            &config,
            "torque",
            PATH_BODY,
            &traces.torque,
            &traces.phases,
            f0,
        );
        println!();
    }
    Ok(())
}
