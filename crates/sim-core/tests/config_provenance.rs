//! Configuration and provenance acceptance tests (SPEC sections 4, 5 and 10).
//!
//! These tests are the guard against an estimate ever being presented as OEM
//! data: every published figure must round-trip unchanged and name a locator,
//! and every parameter must carry exactly one provenance classification.

use std::collections::BTreeSet;

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::config::paths::PARAMETER_PATHS;
use sim_core::config::ProvenanceStatus;
use sim_core::{EngineConfig, ValidatedConfig};

fn parsed() -> EngineConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON).expect("built-in config parses")
}

fn validated() -> ValidatedConfig {
    parsed().validate().expect("built-in config validates")
}

fn status_of(config: &EngineConfig, path: &str) -> ProvenanceStatus {
    config
        .provenance
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| panic!("no provenance entry for `{path}`"))
        .status
}

#[test]
fn schema_version_is_pinned() {
    assert_eq!(parsed().schema_version, sim_core::config::SCHEMA_VERSION);
}

#[test]
fn identity_matches_the_specified_stable_id() {
    let config = parsed();
    assert_eq!(config.identity.id, sim_core::BUILTIN_ENGINE_ID);
    assert_eq!(config.identity.id, "mercedes-benz-om471-9-m3d-375kw");
    assert_eq!(
        config.identity.display_name,
        "OM 471.9 M3D 375 kW Reference"
    );
    assert_eq!(config.identity.power_code, "M3D");
    assert!(
        !config.identity.disclaimer.trim().is_empty(),
        "the model must always carry a fidelity disclaimer"
    );
}

#[test]
fn published_values_round_trip_exactly() {
    let config = parsed();

    // Geometry, from the technical data table.
    assert_eq!(config.geometry.cylinders, 6);
    assert_eq!(config.geometry.bore_m, 0.132);
    assert_eq!(config.geometry.stroke_m, 0.156);
    assert_eq!(config.geometry.connecting_rod_m, 0.268);
    assert_eq!(config.geometry.compression_ratio, 17.3);
    assert_eq!(config.geometry.published_displacement_m3, 0.0128);
    assert_eq!(config.geometry.published_stroke_bore_ratio, 1.18);

    // Valve train: DOHC, 2 intake + 2 exhaust per cylinder.
    assert_eq!(config.valvetrain.intake_valves_per_cylinder, 2);
    assert_eq!(config.valvetrain.exhaust_valves_per_cylinder, 2);

    // Idle speed.
    assert_eq!(config.governor.idle_target_rpm, 560.0);

    // Complete-engine mass: metadata only.
    assert_eq!(config.inertia.complete_engine_mass_kg, 1200.0);

    // M3D rated output.
    assert_eq!(config.rated.max_power_w, 375_000.0);
    assert_eq!(config.rated.max_power_hp, 510.0);
    assert_eq!(config.rated.max_torque_nm, 2500.0);

    // Injection pressures: 900 bar rail, up to 2100 bar amplified.
    assert_eq!(config.injection.rail_pressure_max_pa, 90.0e6);
    assert_eq!(config.injection.amplified_pressure_max_pa, 210.0e6);

    // Combustion-pressure envelope: up to 230 bar.
    assert_eq!(config.limits.max_combustion_pressure_pa, 23.0e6);
}

#[test]
fn published_values_carry_the_published_classification() {
    let config = parsed();
    for path in [
        "geometry.cylinders",
        "geometry.bore_m",
        "geometry.stroke_m",
        "geometry.connecting_rod_m",
        "geometry.compression_ratio",
        "geometry.published_displacement_m3",
        "geometry.published_stroke_bore_ratio",
        "valvetrain.intake_valves_per_cylinder",
        "valvetrain.exhaust_valves_per_cylinder",
        "injection.rail_pressure_max_pa",
        "injection.amplified_pressure_max_pa",
        "governor.idle_target_rpm",
        "inertia.complete_engine_mass_kg",
        "limits.max_combustion_pressure_pa",
        "rated.max_power_w",
        "rated.max_power_hp",
        "rated.max_torque_nm",
    ] {
        assert_eq!(
            status_of(&config, path),
            ProvenanceStatus::Published,
            "`{path}` must be classified as published"
        );
    }
}

#[test]
fn values_absent_from_the_manual_are_never_classified_as_published() {
    let config = parsed();
    // SPEC section 4 "Data gaps": the manual publishes none of these.
    for path in [
        "geometry.firing_order",
        "valvetrain.intake_valve_close_rad",
        "valvetrain.exhaust_valve_open_rad",
        "combustion.cetane_number",
        "combustion.premixed_duration_rad",
        "combustion.diffusion_duration_rad_per_mg",
        "friction.fmep_constant_pa",
        "inertia.rotating_inertia_kg_m2",
        "governor.overspeed_taper_start_rpm",
        "governor.overspeed_cutoff_rpm",
        "air_path.boost_target_schedule",
        "air_path.intercooler_effectiveness",
        "air_path.exhaust_restriction_pa_per_kg2_s2",
        "injection.soi_schedule",
        "injection.soi_load_retard_rad_per_mg",
        "injection.nozzle_hole_diameter_m",
        "heat_transfer.wall_temperature_k",
        // The manual describes the turbocharger architecture and the EGR loop in
        // detail, but supplies no numbers for either beyond the cooler duty.
        "turbo.turbine_effective_area_m2",
        "turbo.compressor_efficiency",
        "turbo.shaft_inertia_kg_m2",
        "turbo.max_shaft_speed_rad_per_s",
        "turbo.wastegate_max_area_m2",
        "egr.rate_schedule",
        "egr.valve_max_area_m2",
        "egr.max_rate",
    ] {
        assert_eq!(
            status_of(&config, path),
            ProvenanceStatus::Calibrated,
            "`{path}` is not published in the manual and must be classified as calibrated"
        );
    }
}

/// The turbocharger section mentions a pressure of "up to 2.8 bar" applied to
/// the wastegate vacuum cell. That is the pneumatic pressure operating the
/// actuator, **not** boost pressure — and it sits close enough to this model's
/// calibrated peak boost to be mistaken for it. Nothing may claim it.
#[test]
fn the_wastegate_actuator_pressure_is_never_mistaken_for_published_boost() {
    let config = parsed();

    let boost = &config.air_path.boost_target_schedule;
    assert_eq!(
        status_of(&config, "air_path.boost_target_schedule"),
        ProvenanceStatus::Calibrated,
        "the manual publishes no boost pressure"
    );
    for value in &boost.values {
        assert!(
            (*value - 280_000.0).abs() > 1.0,
            "boost setpoint {value} Pa is the wastegate control pressure, not boost"
        );
    }

    // And no published entry anywhere may carry that value.
    for entry in &config.provenance {
        if entry.status != ProvenanceStatus::Published {
            continue;
        }
        if let Some(value) = config.value_at(&entry.path) {
            assert!(
                (value - 280_000.0).abs() > 1.0,
                "`{}` claims 2.8 bar as published; that figure is the wastegate \
                 vacuum-cell control pressure, not a boost pressure",
                entry.path
            );
        }
    }
}

/// The engine brake section is the richest in the manual, and it publishes six
/// numbers: the speed floor, the three cylinders of stage I, and the two
/// power anchors for the fitted variant. Everything describing the *shape* of the
/// braking event is ours.
#[test]
fn the_engine_brake_publishes_exactly_six_figures() {
    let config = parsed();

    for path in [
        "engine_brake.min_speed_rpm",
        "engine_brake.stage1_cylinder_count",
        "engine_brake.anchor_low_rpm",
        "engine_brake.anchor_low_power_w",
        "engine_brake.anchor_high_rpm",
        "engine_brake.anchor_high_power_w",
    ] {
        assert_eq!(
            status_of(&config, path),
            ProvenanceStatus::Published,
            "`{path}` is published in the engine brake section"
        );
        let entry = config
            .provenance
            .iter()
            .find(|e| e.path == path)
            .expect("entry");
        let locator = entry.locator.as_deref().unwrap_or("");
        assert!(
            locator.contains("GF14.15-W-0002H"),
            "`{path}` must cite the engine brake section, got `{locator}`"
        );
    }

    // The published values themselves, unchanged.
    let b = &config.engine_brake;
    assert_eq!(b.min_speed_rpm, 1000.0);
    assert_eq!(b.stage1_cylinder_count, 3);
    assert_eq!(b.anchor_low_rpm, 1300.0);
    assert_eq!(b.anchor_low_power_w, 100_000.0);
    assert_eq!(b.anchor_high_rpm, 2300.0);
    assert_eq!(b.anchor_high_power_w, 300_000.0);
    assert_eq!(b.variant, "M5U");

    // Everything about the mechanism is calibrated. The manual publishes no cam
    // contour, no lift, no timing and no MCM target.
    for path in [
        "engine_brake.charge_center_rad",
        "engine_brake.release_center_rad",
        "engine_brake.charge_width_rad",
        "engine_brake.release_width_rad",
        "engine_brake.effective_area_m2",
        "engine_brake.stage1_boost_target_pa",
        "engine_brake.stage2_boost_target_pa",
        "engine_brake.stage3_boost_target_pa",
        "engine_brake.wastegate_gain_scale",
        "engine_brake.stage1_egr_command",
        "engine_brake.stage2_egr_command",
        "engine_brake.stage3_egr_command",
    ] {
        assert_eq!(
            status_of(&config, path),
            ProvenanceStatus::Calibrated,
            "`{path}` is not published and must be classified as calibrated"
        );
    }
}

/// The M5V figures belong to a variant this configuration is not fitted with.
///
/// The manual publishes 150 kW / 400 kW for the high performance system. This
/// engine carries the standard M5U record, and mixing the two would be exactly
/// the kind of cross-contamination SPEC section 4 forbids for the later 390 kW
/// figures.
#[test]
fn the_high_performance_brake_figures_do_not_leak_into_the_standard_variant() {
    let config = parsed();
    assert_eq!(config.engine_brake.variant, "M5U");
    for value in [150_000.0, 400_000.0] {
        assert!(
            (config.engine_brake.anchor_low_power_w - value).abs() > 1.0
                && (config.engine_brake.anchor_high_power_w - value).abs() > 1.0,
            "{value} W belongs to the M5V high performance record, not to M5U"
        );
    }
}

/// The published brake anchors are a validation target, and a model allowed to
/// read its own answer proves nothing.
///
/// This is the same guard the 2.8 bar test applies, aimed at a different trap:
/// there, a number that must never be *claimed*; here, numbers that must never
/// be *used*. Braking torque has to emerge from cylinder pressure through
/// slider-crank geometry exactly as firing torque does, so no solver source may
/// so much as mention the anchor fields.
#[test]
fn no_solver_source_reads_the_published_brake_anchors() {
    const SOLVER_SOURCES: &[(&str, &str)] = &[
        ("sim/mod.rs", include_str!("../src/sim/mod.rs")),
        ("sim/step.rs", include_str!("../src/sim/step.rs")),
        ("sim/brake.rs", include_str!("../src/sim/brake.rs")),
        ("sim/driveline.rs", include_str!("../src/sim/driveline.rs")),
        ("sim/torque.rs", include_str!("../src/sim/torque.rs")),
        ("sim/turbo.rs", include_str!("../src/sim/turbo.rs")),
        ("sim/flow.rs", include_str!("../src/sim/flow.rs")),
        ("sim/acoustics.rs", include_str!("../src/sim/acoustics.rs")),
        ("dyno.rs", include_str!("../src/dyno.rs")),
    ];

    for (name, source) in SOLVER_SOURCES {
        // Only production code is scanned. A module's own unit tests legitimately
        // build an `EngineBrake` literal, anchors and all, and that is not the
        // solver consulting them.
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for (number, line) in production.lines().enumerate() {
            // The brake module's own doc comment says these must not be read;
            // saying so is not reading them.
            let code = line.split("//").next().unwrap_or("");
            for field in [
                "anchor_low_rpm",
                "anchor_low_power_w",
                "anchor_high_rpm",
                "anchor_high_power_w",
            ] {
                assert!(
                    !code.contains(field),
                    "{name}:{} reads `{field}`. The published brake anchors are a \
                     validation target; braking torque must come out of cylinder \
                     pressure, not out of the answer sheet.",
                    number + 1
                );
            }
        }
    }
}

/// The EGR cooler duty is the one thing Milestone 3 adds that the manual
/// really does publish, and the effectiveness the solver uses is derived from
/// it rather than invented alongside it.
#[test]
fn the_egr_cooler_temperatures_are_published_and_the_effectiveness_follows_them() {
    let config = parsed();

    assert_eq!(
        status_of(&config, "egr.cooler_inlet_temperature_k"),
        ProvenanceStatus::Published
    );
    assert_eq!(
        status_of(&config, "egr.cooler_outlet_temperature_k"),
        ProvenanceStatus::Published
    );
    assert_eq!(
        status_of(&config, "egr.cooler_effectiveness"),
        ProvenanceStatus::Derived
    );

    // 650 C and 170 C, as printed.
    assert!((config.egr.cooler_inlet_temperature_k - 923.15).abs() < 1.0e-9);
    assert!((config.egr.cooler_outlet_temperature_k - 443.15).abs() < 1.0e-9);

    let expected = (config.egr.cooler_inlet_temperature_k - config.egr.cooler_outlet_temperature_k)
        / (config.egr.cooler_inlet_temperature_k - config.air_path.coolant_temperature_k);
    assert!(
        (config.egr.cooler_effectiveness - expected).abs() < 1.0e-6,
        "effectiveness {} does not follow from the published pair ({expected})",
        config.egr.cooler_effectiveness
    );
}

#[test]
fn provenance_covers_every_parameter_path_exactly_once() {
    let config = parsed();
    let expected: BTreeSet<&str> = PARAMETER_PATHS.iter().copied().collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in &config.provenance {
        assert!(
            expected.contains(entry.path.as_str()),
            "provenance names unknown path `{}`",
            entry.path
        );
        assert!(
            seen.insert(entry.path.as_str()),
            "duplicate provenance for `{}`",
            entry.path
        );
    }
    let missing: Vec<&str> = expected.difference(&seen).copied().collect();
    assert!(missing.is_empty(), "missing provenance for: {missing:?}");
}

#[test]
fn published_entries_name_a_real_source_and_locator() {
    let config = parsed();
    let source_ids: BTreeSet<&str> = config.sources.iter().map(|s| s.id.as_str()).collect();
    for entry in config.provenance.iter().filter(|e| e.is_published()) {
        let source_id = entry
            .source_id
            .as_deref()
            .unwrap_or_else(|| panic!("published `{}` has no source_id", entry.path));
        assert!(
            source_ids.contains(source_id),
            "published `{}` names unknown source `{source_id}`",
            entry.path
        );
        let locator = entry
            .locator
            .as_deref()
            .unwrap_or_else(|| panic!("published `{}` has no locator", entry.path));
        assert!(
            !locator.trim().is_empty(),
            "published `{}` has an empty locator",
            entry.path
        );
    }
}

#[test]
fn derived_entries_state_a_formula_and_inputs() {
    let config = parsed();
    let derived: Vec<_> = config
        .provenance
        .iter()
        .filter(|e| e.is_derived())
        .collect();
    assert!(
        !derived.is_empty(),
        "at least one value should be derived rather than asserted"
    );
    for entry in derived {
        assert!(
            entry
                .formula
                .as_deref()
                .is_some_and(|f| !f.trim().is_empty()),
            "derived `{}` must state its formula",
            entry.path
        );
        assert!(
            !entry.inputs.is_empty(),
            "derived `{}` must list its inputs",
            entry.path
        );
    }
}

#[test]
fn calibrated_entries_state_a_purpose_and_a_range_containing_the_value() {
    let config = parsed();
    for entry in config.provenance.iter().filter(|e| e.is_calibrated()) {
        assert!(
            entry
                .purpose
                .as_deref()
                .is_some_and(|p| !p.trim().is_empty()),
            "calibrated `{}` must state its purpose",
            entry.path
        );
        let range = entry
            .safe_range
            .unwrap_or_else(|| panic!("calibrated `{}` must give a safe_range", entry.path));
        assert!(
            range[0] <= range[1],
            "`{}` has a reversed range",
            entry.path
        );
        if let Some(value) = config.value_at(&entry.path) {
            assert!(
                (range[0]..=range[1]).contains(&value),
                "calibrated `{}` value {value} lies outside {range:?}",
                entry.path
            );
        }
    }
}

#[test]
fn the_source_record_identifies_the_om471_manual() {
    let config = parsed();
    let source = config
        .sources
        .iter()
        .find(|s| s.id == "mb-om471-intro-2011")
        .expect("the OM 471 manual must be a source record");
    assert_eq!(
        source.title,
        "Introduction of engine OM 471 and exhaust aftertreatment"
    );
    assert_eq!(source.technical_status, "2011-09-01");
    assert_eq!(source.order_number, "6517 1260 02");
    assert!(
        source.scope.contains("471.9"),
        "source scope must state which engine series it covers"
    );
}

#[test]
fn later_om471_figures_are_not_present() {
    // SPEC section 4: do not mix the later 390 kW / 2600 Nm / 2700 bar figures in.
    let config = parsed();
    assert_ne!(config.rated.max_power_w, 390_000.0);
    assert_ne!(config.rated.max_torque_nm, 2600.0);
    assert_ne!(config.injection.amplified_pressure_max_pa, 270.0e6);
}

#[test]
fn provenance_report_counts_every_entry() {
    let report = validated().provenance_report();
    assert_eq!(report.entries.len(), PARAMETER_PATHS.len());
    assert_eq!(
        report.published_count + report.derived_count + report.calibrated_count,
        PARAMETER_PATHS.len()
    );
    assert!(report.published_count > 0 && report.calibrated_count > 0);
    assert!(!report.disclaimer.trim().is_empty());
    assert_eq!(report.config_id, sim_core::BUILTIN_ENGINE_ID);
}

// --- negative cases: the validator must reject malformed provenance ---

fn mutate(f: impl FnOnce(&mut serde_json::Value)) -> sim_core::SimError {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("built-in config parses as JSON");
    f(&mut document);
    EngineConfig::from_json(&document.to_string())
        .expect("mutated config still parses")
        .validate()
        .expect_err("mutated config must fail validation")
}

fn entry_index(document: &serde_json::Value, path: &str) -> usize {
    document["provenance"]
        .as_array()
        .unwrap()
        .iter()
        .position(|e| e["path"] == path)
        .unwrap()
}

#[test]
fn rejects_a_published_value_without_a_locator() {
    let error = mutate(|d| {
        let index = entry_index(d, "geometry.bore_m");
        d["provenance"][index]["locator"] = serde_json::Value::Null;
    });
    assert_eq!(error.code, sim_core::ErrorCode::InvalidConfig);
    assert!(error.message.contains("locator"), "{}", error.message);
}

#[test]
fn rejects_a_published_value_naming_an_unknown_source() {
    let error = mutate(|d| {
        let index = entry_index(d, "geometry.stroke_m");
        d["provenance"][index]["source_id"] = serde_json::json!("no-such-source");
    });
    assert!(
        error.message.contains("unknown source_id"),
        "{}",
        error.message
    );
}

#[test]
fn rejects_a_calibrated_value_outside_its_safe_range() {
    let error = mutate(|d| {
        d["inertia"]["rotating_inertia_kg_m2"] = serde_json::json!(999.0);
    });
    assert!(error.message.contains("safe_range"), "{}", error.message);
}

#[test]
fn rejects_a_derived_value_without_inputs() {
    let error = mutate(|d| {
        let index = entry_index(d, "gas.gas_constant_j_per_kg_k");
        d["provenance"][index]["inputs"] = serde_json::json!([]);
    });
    assert!(error.message.contains("inputs"), "{}", error.message);
}

#[test]
fn rejects_a_missing_provenance_entry() {
    let error = mutate(|d| {
        let index = entry_index(d, "solver.fixed_step_s");
        d["provenance"].as_array_mut().unwrap().remove(index);
    });
    assert!(
        error.message.contains("missing provenance")
            && error.message.contains("solver.fixed_step_s"),
        "{}",
        error.message
    );
}

#[test]
fn rejects_an_unsupported_schema_version() {
    let error = mutate(|d| d["schema_version"] = serde_json::json!(99));
    assert!(
        error.message.contains("schema_version"),
        "{}",
        error.message
    );
}

#[test]
fn rejects_geometry_that_contradicts_the_published_displacement() {
    let error = mutate(|d| d["geometry"]["bore_m"] = serde_json::json!(0.150));
    assert!(error.message.contains("displacement"), "{}", error.message);
}

#[test]
fn rejects_a_firing_order_that_is_not_a_permutation() {
    let error = mutate(|d| d["geometry"]["firing_order"] = serde_json::json!([1, 1, 3, 6, 2, 4]));
    assert!(error.message.contains("permutation"), "{}", error.message);
}
