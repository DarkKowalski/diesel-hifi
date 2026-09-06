//! Geometry acceptance tests (README "Acceptance criteria").

use std::f64::consts::PI;

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::geometry::SliderCrank;
use sim_core::EngineConfig;

fn om471() -> sim_core::ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("built-in config parses")
        .validate()
        .expect("built-in config validates")
}

#[test]
fn recomputed_displacement_matches_published_12_8_litres() {
    let config = om471();
    let geometry = &config.config().geometry;

    assert_eq!(geometry.cylinders, 6);
    assert_eq!(geometry.bore_m, 0.132);
    assert_eq!(geometry.stroke_m, 0.156);

    // Recompute from six cylinders and 132 x 156 mm geometry.
    let recomputed_l = PI
        * 0.25
        * geometry.bore_m
        * geometry.bore_m
        * geometry.stroke_m
        * geometry.cylinders as f64
        * 1000.0;

    assert!(
        (recomputed_l - 12.8).abs() <= 0.05,
        "recomputed {recomputed_l:.4} L must match published 12.8 L within 0.05 L"
    );
    assert!((config.derived().total_displacement_m3 * 1000.0 - recomputed_l).abs() < 1.0e-9);
}

#[test]
fn derived_stroke_bore_ratio_matches_published_value() {
    let config = om471();
    let published = config.config().geometry.published_stroke_bore_ratio;
    let derived = config.derived().stroke_bore_ratio;
    assert!(
        (derived - published).abs() <= 0.005,
        "derived stroke:bore {derived:.4} must match published {published:.2}"
    );
}

#[test]
fn clearance_volume_reproduces_the_compression_ratio() {
    let config = om471();
    let d = config.derived();
    let ratio = d.max_volume_m3 / d.clearance_volume_m3;
    assert!(
        (ratio - config.config().geometry.compression_ratio).abs() < 1.0e-9,
        "V_max / V_clearance must equal the configured compression ratio, got {ratio}"
    );
}

#[test]
fn slider_crank_hits_the_expected_dead_centres() {
    let config = om471();
    let slider =
        SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m);
    let stroke = config.config().geometry.stroke_m;

    // Top dead centre: no displacement, volume equals the clearance volume.
    assert!(slider.piston_displacement_m(0.0).abs() < 1.0e-12);
    assert!((slider.volume_m3(0.0) - config.derived().clearance_volume_m3).abs() < 1.0e-15);

    // Bottom dead centre: displacement equals the full stroke.
    let bdc = slider.piston_displacement_m(PI);
    assert!(
        (bdc - stroke).abs() < 1.0e-12,
        "piston displacement at BDC should equal the stroke, got {bdc}"
    );
    assert!((slider.volume_m3(PI) - config.derived().max_volume_m3).abs() < 1.0e-15);

    // A full revolution returns to top dead centre.
    assert!((slider.volume_m3(2.0 * PI) - slider.volume_m3(0.0)).abs() < 1.0e-15);
}

#[test]
fn dvolume_dtheta_changes_sign_at_both_dead_centres() {
    let config = om471();
    let slider =
        SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m);

    // Zero at both dead centres.
    assert!(slider.dvolume_dtheta_m3_per_rad(0.0).abs() < 1.0e-15);
    assert!(slider.dvolume_dtheta_m3_per_rad(PI).abs() < 1.0e-15);

    // Positive while descending, negative while rising.
    assert!(slider.dvolume_dtheta_m3_per_rad(0.5 * PI) > 0.0);
    assert!(slider.dvolume_dtheta_m3_per_rad(1.5 * PI) < 0.0);
}

#[test]
fn dvolume_dtheta_agrees_with_a_numerical_derivative() {
    let config = om471();
    let slider =
        SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m);

    let h = 1.0e-6;
    for step in 1..36 {
        let theta = step as f64 * PI / 18.0;
        let numeric = (slider.volume_m3(theta + h) - slider.volume_m3(theta - h)) / (2.0 * h);
        let analytic = slider.dvolume_dtheta_m3_per_rad(theta);
        assert!(
            (numeric - analytic).abs() < 1.0e-12,
            "at theta={theta}: analytic {analytic} vs numeric {numeric}"
        );
    }
}

#[test]
fn firing_order_produces_evenly_spaced_phase_offsets() {
    let config = om471();
    let offsets = &config.derived().phase_offsets_rad;
    assert_eq!(offsets.len(), 6);

    // Cylinder 1 fires first; the published-order-independent property is that
    // offsets are a permutation of evenly spaced firing intervals.
    let interval = 4.0 * PI / 6.0;
    let mut sorted = offsets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for (index, offset) in sorted.iter().enumerate() {
        assert!(
            (offset - index as f64 * interval).abs() < 1.0e-12,
            "phase offsets must be evenly spaced by {interval} rad"
        );
    }
    assert_eq!(offsets[0], 0.0, "cylinder 1 is the phase reference");
}
