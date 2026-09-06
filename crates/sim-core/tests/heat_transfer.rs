//! Woschni wall heat-transfer tests.
//!
//! Milestone 1 folded wall heat loss into fixed polytropic exponents. These
//! tests exist to prove the explicit term that replaced it is real and wired in,
//! not merely present in the source.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::geometry::SliderCrank;
use sim_core::sim::heat_transfer;
use sim_core::{Catalog, Controls, Engine, EngineConfig, ResetOptions, ValidatedConfig};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

fn slider(config: &ValidatedConfig) -> SliderCrank {
    SliderCrank::from_derived(config.derived(), config.config().geometry.connecting_rod_m)
}

#[test]
fn the_coefficient_is_positive_and_rises_with_pressure() {
    let low = heat_transfer::coefficient_w_per_m2_k(0.132, 4.0e6, 900.0, 10.0);
    let high = heat_transfer::coefficient_w_per_m2_k(0.132, 14.0e6, 900.0, 10.0);
    assert!(low > 0.0);
    assert!(
        high > low,
        "a denser charge must transfer heat faster: {high} vs {low}"
    );
}

#[test]
fn the_coefficient_rises_with_gas_velocity() {
    let slow = heat_transfer::coefficient_w_per_m2_k(0.132, 10.0e6, 1200.0, 5.0);
    let fast = heat_transfer::coefficient_w_per_m2_k(0.132, 10.0e6, 1200.0, 20.0);
    assert!(fast > slow);
}

#[test]
fn the_coefficient_is_zero_for_degenerate_inputs() {
    assert_eq!(
        heat_transfer::coefficient_w_per_m2_k(0.132, 10.0e6, 1200.0, 0.0),
        0.0
    );
    assert_eq!(
        heat_transfer::coefficient_w_per_m2_k(0.132, 0.0, 1200.0, 10.0),
        0.0
    );
}

#[test]
fn the_combustion_term_only_acts_while_the_cylinder_is_closed() {
    let config = config();
    let ht = &config.config().heat_transfer;
    let derived = config.derived();

    let closed = heat_transfer::gas_velocity_m_per_s(
        ht,
        true,
        7.0,
        14.0e6,
        6.0e6,
        derived.displacement_per_cylinder_m3,
        2.6e5,
        330.0,
        derived.max_volume_m3,
    );
    let open = heat_transfer::gas_velocity_m_per_s(
        ht,
        false,
        7.0,
        14.0e6,
        6.0e6,
        derived.displacement_per_cylinder_m3,
        2.6e5,
        330.0,
        derived.max_volume_m3,
    );

    // Gas exchange uses the larger C1 but carries no combustion term.
    assert!((open - ht.woschni_c1_gas_exchange * 7.0).abs() < 1.0e-12);
    assert!(
        closed > ht.woschni_c1_closed * 7.0,
        "a fired cylinder must add combustion-driven velocity"
    );
}

#[test]
fn chamber_area_grows_as_the_piston_descends() {
    let config = config();
    let slider = slider(&config);
    let bore = config.config().geometry.bore_m;

    let tdc = heat_transfer::chamber_area_m2(&slider, bore, 0.0);
    let bdc = heat_transfer::chamber_area_m2(&slider, bore, std::f64::consts::PI);
    assert!(tdc > 0.0);
    assert!(bdc > tdc, "the liner is exposed as the piston descends");

    // At TDC the area is just the head and the crown.
    let expected_tdc = 2.0 * std::f64::consts::PI * 0.25 * bore * bore;
    assert!((tdc - expected_tdc).abs() < 1.0e-12);
}

#[test]
fn hot_gas_loses_heat_and_the_term_can_be_disabled() {
    let config = config();
    let slider = slider(&config);
    let bore = config.config().geometry.bore_m;
    let mut ht = config.config().heat_transfer;

    let loss =
        heat_transfer::heat_loss_j(&ht, &slider, bore, 0.2, 12.0e6, 1800.0, 12.0, 150.0, 0.006);
    assert!(loss > 0.0, "gas above wall temperature must lose heat");

    // Below wall temperature the sign flips: the wall warms the gas.
    let gain =
        heat_transfer::heat_loss_j(&ht, &slider, bore, 0.2, 12.0e6, 350.0, 12.0, 150.0, 0.006);
    assert!(gain < 0.0);

    ht.enabled = false;
    assert_eq!(
        heat_transfer::heat_loss_j(&ht, &slider, bore, 0.2, 12.0e6, 1800.0, 12.0, 150.0, 0.006),
        0.0
    );
}

#[test]
fn disabling_heat_transfer_raises_peak_motored_pressure() {
    // A motored engine with no wall losses must compress to a higher peak. This
    // is what proves the term is actually connected to the solver, not just
    // present in the module.
    fn peak_pressure(enabled: bool) -> f64 {
        let mut document: serde_json::Value = serde_json::from_str(OM471_9_M3D_JSON).unwrap();
        document["heat_transfer"]["enabled"] = serde_json::json!(enabled);
        let catalog =
            Catalog::from_documents(&[&document.to_string()]).expect("document validates");
        let mut engine =
            Engine::from_catalog(catalog, ResetOptions::default()).expect("engine builds");
        // Motored: crank it over with the starter, never fuel it.
        engine
            .set_controls(Controls {
                pedal: 0.0,
                load_torque_nm: 0.0,
                starter: true,
                ignition: false,
                egr_enabled: true,
            })
            .expect("controls accepted");
        let mut snapshot = engine.snapshot();
        for _ in 0..40 {
            snapshot = engine.advance(4_000).expect("advance");
        }
        assert!(snapshot.rpm > 100.0, "the motored engine should be turning");
        snapshot.peak_pressure_pa_session
    }

    let with_loss = peak_pressure(true);
    let without_loss = peak_pressure(false);
    assert!(
        without_loss > with_loss,
        "removing wall heat loss must raise peak motored pressure: \
         {without_loss:.0} Pa vs {with_loss:.0} Pa"
    );
}

#[test]
fn the_running_engine_reports_a_plausible_residual_fraction() {
    let mut engine = Engine::builtin().expect("catalog builds");
    engine
        .set_controls(Controls {
            pedal: 0.6,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
        })
        .expect("controls accepted");
    for tick in 0..80 {
        engine.advance(4_000).expect("advance");
        if tick == 20 {
            engine
                .set_controls(Controls {
                    pedal: 0.6,
                    load_torque_nm: 900.0,
                    starter: false,
                    ignition: true,
                    egr_enabled: true,
                })
                .expect("controls accepted");
        }
    }
    let residual = engine.snapshot().residual_fraction;
    assert!(
        (0.0..0.20).contains(&residual),
        "residual fraction {residual:.4} is outside the plausible band"
    );
    assert!(
        residual > 0.0,
        "some burned gas must survive the exhaust stroke"
    );
}
