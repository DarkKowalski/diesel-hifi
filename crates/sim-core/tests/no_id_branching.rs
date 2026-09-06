//! README "Configuration and provenance": "Solver code must not branch on the OM 471
//! ID."
//!
//! This is a source-level guard. The solver modules are compiled into the test
//! binary as text and checked for engine-specific identifiers. If someone adds
//! `if id == "mercedes-benz-om471-..."` to the physics, this fails.

/// Every module that participates in stepping the simulation.
const SOLVER_SOURCES: &[(&str, &str)] = &[
    ("sim/mod.rs", include_str!("../src/sim/mod.rs")),
    ("sim/step.rs", include_str!("../src/sim/step.rs")),
    ("sim/cylinder.rs", include_str!("../src/sim/cylinder.rs")),
    ("sim/torque.rs", include_str!("../src/sim/torque.rs")),
    ("sim/governor.rs", include_str!("../src/sim/governor.rs")),
    ("sim/gas.rs", include_str!("../src/sim/gas.rs")),
    ("sim/injection.rs", include_str!("../src/sim/injection.rs")),
    (
        "sim/ignition_delay.rs",
        include_str!("../src/sim/ignition_delay.rs"),
    ),
    (
        "sim/heat_transfer.rs",
        include_str!("../src/sim/heat_transfer.rs"),
    ),
    (
        "sim/heat_release.rs",
        include_str!("../src/sim/heat_release.rs"),
    ),
    ("sim/acoustics.rs", include_str!("../src/sim/acoustics.rs")),
    ("sim/brake.rs", include_str!("../src/sim/brake.rs")),
    ("sim/driveline.rs", include_str!("../src/sim/driveline.rs")),
    ("sim/egr.rs", include_str!("../src/sim/egr.rs")),
    ("sim/flow.rs", include_str!("../src/sim/flow.rs")),
    ("sim/manifold.rs", include_str!("../src/sim/manifold.rs")),
    ("sim/turbo.rs", include_str!("../src/sim/turbo.rs")),
    ("geometry.rs", include_str!("../src/geometry.rs")),
    // The dynamometer harness reads whatever configuration it is handed, so it
    // must be free of engine identity too.
    ("dyno.rs", include_str!("../src/dyno.rs")),
];

/// Identifiers that must never appear in solver code.
const FORBIDDEN: &[&str] = &["mercedes", "om471", "om 471", "m3d", "471.9", "daimler"];

#[test]
fn solver_code_never_mentions_an_engine_identity() {
    for (name, source) in SOLVER_SOURCES {
        let lowered = source.to_lowercase();
        for needle in FORBIDDEN {
            assert!(
                !lowered.contains(needle),
                "solver module `{name}` mentions `{needle}`; engine-specific values must arrive \
                 through ValidatedConfig, never be branched on in the solver"
            );
        }
    }
}

/// The stepping and geometry math must not read the configuration identity at
/// all. `sim/mod.rs` is excluded here because it builds the snapshot, which
/// legitimately labels telemetry with the active configuration ID.
const STEPPING_SOURCES: &[(&str, &str)] = &[
    ("sim/step.rs", include_str!("../src/sim/step.rs")),
    ("sim/cylinder.rs", include_str!("../src/sim/cylinder.rs")),
    ("sim/torque.rs", include_str!("../src/sim/torque.rs")),
    ("sim/governor.rs", include_str!("../src/sim/governor.rs")),
    ("sim/gas.rs", include_str!("../src/sim/gas.rs")),
    ("sim/injection.rs", include_str!("../src/sim/injection.rs")),
    (
        "sim/ignition_delay.rs",
        include_str!("../src/sim/ignition_delay.rs"),
    ),
    (
        "sim/heat_transfer.rs",
        include_str!("../src/sim/heat_transfer.rs"),
    ),
    (
        "sim/heat_release.rs",
        include_str!("../src/sim/heat_release.rs"),
    ),
    ("sim/acoustics.rs", include_str!("../src/sim/acoustics.rs")),
    ("sim/brake.rs", include_str!("../src/sim/brake.rs")),
    ("sim/driveline.rs", include_str!("../src/sim/driveline.rs")),
    ("sim/egr.rs", include_str!("../src/sim/egr.rs")),
    ("sim/flow.rs", include_str!("../src/sim/flow.rs")),
    ("sim/manifold.rs", include_str!("../src/sim/manifold.rs")),
    ("sim/turbo.rs", include_str!("../src/sim/turbo.rs")),
    ("geometry.rs", include_str!("../src/geometry.rs")),
];

#[test]
fn stepping_code_never_reads_the_configuration_identity() {
    for (name, source) in STEPPING_SOURCES {
        for needle in ["BUILTIN_ENGINE_ID", "identity.id", "identity"] {
            assert!(
                !source.contains(needle),
                "stepping module `{name}` mentions `{needle}`; the solver must read only \
                 numeric configuration, never the engine identity"
            );
        }
    }
}

/// `sim/mod.rs` may name the identity only to label a snapshot, never to branch.
#[test]
fn the_only_identity_use_in_the_simulation_module_is_snapshot_labelling() {
    let source = include_str!("../src/sim/mod.rs");
    let uses: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("identity"))
        .collect();
    assert_eq!(
        uses,
        vec!["config_id: cfg.identity.id.clone(),"],
        "the simulation module may only read the identity to label a snapshot"
    );
}
