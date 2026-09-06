//! Injection, ignition delay, and double-Wiebe heat-release tests.
//!
//! These check the Milestone 2 combustion chain in isolation: the injector
//! decides how long fuel takes to deliver, the delay correlation decides when it
//! lights, and the two together decide how much of the burn is premixed.

use std::f64::consts::PI;

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::config::Combustion;
use sim_core::sim::heat_release::{self, Profile};
use sim_core::sim::ignition_delay::{activation_energy_j_per_mol, ignition_delay_rad};
use sim_core::sim::injection::{self, Variant};
use sim_core::{Controls, Engine, EngineConfig, ValidatedConfig};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

// --- ignition delay ---------------------------------------------------------

#[test]
fn activation_energy_falls_as_cetane_number_rises() {
    assert!(activation_energy_j_per_mol(40.0) > activation_energy_j_per_mol(55.0));
}

#[test]
fn ignition_delay_lengthens_as_the_charge_gets_colder() {
    let hot = ignition_delay_rad(50.0, 10.0e6, 950.0, 6.0);
    let cold = ignition_delay_rad(50.0, 10.0e6, 700.0, 6.0);
    assert!(
        cold > hot,
        "a colder charge must take longer to ignite: {cold} vs {hot}"
    );
}

#[test]
fn ignition_delay_shortens_as_the_charge_gets_denser() {
    let low_boost = ignition_delay_rad(50.0, 5.0e6, 850.0, 6.0);
    let high_boost = ignition_delay_rad(50.0, 14.0e6, 850.0, 6.0);
    assert!(
        high_boost < low_boost,
        "a denser charge must ignite sooner: {high_boost} vs {low_boost}"
    );
}

#[test]
fn ignition_delay_stays_in_a_physically_sane_band() {
    // Full-load, fully boosted conditions.
    let delay = ignition_delay_rad(50.0, 12.0e6, 880.0, 7.3);
    let degrees = delay * 180.0 / PI;
    assert!(
        (0.5..6.0).contains(&degrees),
        "full-load ignition delay {degrees:.2} deg is outside the plausible band"
    );

    // Cranking: cold and low pressure. Still clamped, never absurd.
    let cranking = ignition_delay_rad(50.0, 2.0e6, 600.0, 1.0) * 180.0 / PI;
    assert!(cranking.is_finite() && cranking <= 40.0);
    assert!(cranking > degrees, "cranking delay should exceed full-load");
}

// --- injection --------------------------------------------------------------

#[test]
fn the_published_amplified_variant_is_selected_at_high_fuelling() {
    let config = config();
    let inj = &config.config().injection;
    assert_eq!(injection::select_variant(inj, 10.0), Variant::Standard);
    assert_eq!(injection::select_variant(inj, 300.0), Variant::Amplified);

    // The published pressures are what the variants map to.
    assert_eq!(
        injection::injection_pressure_pa(inj, Variant::Standard),
        90.0e6
    );
    assert_eq!(
        injection::injection_pressure_pa(inj, Variant::Amplified),
        210.0e6
    );
}

#[test]
fn amplified_pressure_delivers_fuel_faster_than_rail_pressure() {
    let config = config();
    let inj = &config.config().injection;
    let standard = injection::mass_flow_kg_per_s(inj, 90.0e6, 8.0e6);
    let amplified = injection::mass_flow_kg_per_s(inj, 210.0e6, 8.0e6);
    assert!(
        amplified > standard * 1.4,
        "amplification must materially raise nozzle flow: {amplified} vs {standard}"
    );
}

#[test]
fn injection_duration_scales_with_quantity_and_engine_speed() {
    let config = config();
    let inj = &config.config().injection;

    let small = injection::schedule(inj, 50.0e-6, 1000.0, 104.7, 5.0e6);
    let large = injection::schedule(inj, 250.0e-6, 1000.0, 104.7, 5.0e6);
    assert!(large.duration_rad > small.duration_rad);
    assert_eq!(large.variant, Variant::Amplified);
    assert_eq!(small.variant, Variant::Standard);

    // Same fuel mass at twice the speed occupies twice the crank angle.
    let slow = injection::schedule(inj, 200.0e-6, 1000.0, 104.7, 5.0e6);
    let fast = injection::schedule(inj, 200.0e-6, 2000.0, 209.4, 5.0e6);
    assert!(
        (fast.duration_rad / slow.duration_rad - 2.0).abs() < 0.05,
        "crank-angle duration should double with speed: {} vs {}",
        fast.duration_rad,
        slow.duration_rad
    );
}

#[test]
fn injection_retards_as_fuelling_rises() {
    let config = config();
    let inj = &config.config().injection;
    let light = injection::start_of_injection_rad(inj, 1400.0, 20.0);
    let heavy = injection::start_of_injection_rad(inj, 1400.0, 280.0);
    assert!(
        heavy > light,
        "high load must retard injection toward TDC: {heavy} vs {light}"
    );
    assert!(
        light < 0.0,
        "light-load injection should still be before TDC"
    );
}

#[test]
fn a_scheduled_event_delivers_its_whole_charge_and_no_more() {
    let config = config();
    let inj = &config.config().injection;
    let event = injection::schedule(inj, 200.0e-6, 1400.0, 146.6, 6.0e6);

    assert_eq!(event.delivered_kg(event.start_of_injection_rad - 0.1), 0.0);
    let half = event.delivered_kg(event.start_of_injection_rad + event.duration_rad * 0.5);
    assert!((half / event.fuel_kg - 0.5).abs() < 1.0e-9);
    let all = event.delivered_kg(event.start_of_injection_rad + event.duration_rad * 5.0);
    assert!((all - event.fuel_kg).abs() < 1.0e-15);
}

// --- heat release -----------------------------------------------------------

fn profile_for(
    combustion: &Combustion,
    fuel_kg: f64,
    delay_rad: f64,
    config: &ValidatedConfig,
) -> Profile {
    let inj = &config.config().injection;
    let event = injection::schedule(inj, fuel_kg, 1400.0, 146.6, 6.0e6);
    heat_release::profile(
        combustion,
        &event,
        delay_rad,
        inj.fuel_lower_heating_value_j_per_kg,
    )
}

#[test]
fn the_premixed_fraction_is_larger_at_light_load() {
    let config = config();
    let combustion = &config.config().combustion;

    // Light load: a short injection finishes inside a long ignition delay, so
    // essentially all the fuel is premixed.
    let light = profile_for(combustion, 15.0e-6, 0.05, &config);
    // Full load: a long injection barely starts before ignition, so the burn is
    // diffusion dominated.
    let heavy = profile_for(combustion, 265.0e-6, 0.03, &config);

    assert!(
        light.premixed_fraction > heavy.premixed_fraction,
        "light load {} should be more premixed than full load {}",
        light.premixed_fraction,
        heavy.premixed_fraction
    );
    assert!(light.premixed_fraction > 0.5);
    assert!(
        heavy.premixed_fraction < 0.3,
        "full load should be diffusion dominated, got {}",
        heavy.premixed_fraction
    );
}

#[test]
fn the_premixed_fraction_is_capped() {
    let config = config();
    let combustion = &config.config().combustion;
    // An absurdly long delay would otherwise premix everything.
    let profile = profile_for(combustion, 20.0e-6, 2.0, &config);
    assert!(profile.premixed_fraction <= combustion.max_premixed_fraction + 1.0e-12);
}

#[test]
fn the_diffusion_burn_lengthens_with_injected_quantity() {
    let config = config();
    let combustion = &config.config().combustion;
    let small = profile_for(combustion, 40.0e-6, 0.05, &config);
    let large = profile_for(combustion, 265.0e-6, 0.05, &config);
    assert!(large.diffusion_duration_rad > small.diffusion_duration_rad);
    assert!(small.diffusion_duration_rad >= combustion.diffusion_duration_min_rad);
}

#[test]
fn cumulative_burn_is_monotonic_and_completes() {
    let config = config();
    let combustion = &config.config().combustion;
    let profile = profile_for(combustion, 200.0e-6, 0.04, &config);

    assert_eq!(
        heat_release::burned_fraction(combustion, &profile, profile.start_of_combustion_rad - 0.1),
        0.0
    );

    let mut previous = 0.0;
    for step in 0..400 {
        let psi = profile.start_of_combustion_rad + step as f64 * 0.005;
        let fraction = heat_release::burned_fraction(combustion, &profile, psi);
        assert!(
            fraction >= previous - 1.0e-15,
            "burned fraction must never decrease: {fraction} after {previous}"
        );
        assert!((0.0..=1.0).contains(&fraction));
        previous = fraction;
    }
    assert!(
        previous >= 0.99,
        "the burn should be essentially complete, reached {previous}"
    );
}

#[test]
fn integrated_heat_release_equals_the_available_chemical_energy() {
    let config = config();
    let combustion = &config.config().combustion;
    let profile = profile_for(combustion, 200.0e-6, 0.04, &config);

    let mut released = 0.0;
    let mut fraction = 0.0;
    for step in 0..4000 {
        let psi = profile.start_of_combustion_rad + step as f64 * 0.0005;
        let (heat, next) = heat_release::heat_release_j(combustion, &profile, fraction, psi);
        released += heat;
        fraction = next;
    }

    let expected = 200.0e-6
        * config.config().injection.fuel_lower_heating_value_j_per_kg
        * combustion.combustion_efficiency;
    assert!(
        (released - expected).abs() / expected < 0.005,
        "integrated heat {released:.1} J should match {expected:.1} J within 0.5%"
    );
}

#[test]
fn a_non_advancing_cycle_angle_releases_no_extra_heat() {
    let config = config();
    let combustion = &config.config().combustion;
    let profile = profile_for(combustion, 200.0e-6, 0.04, &config);

    // Simulate the gas-exchange discontinuity: psi jumps backwards.
    let (_, fraction) = heat_release::heat_release_j(combustion, &profile, 0.0, 10.0);
    assert!(fraction > 0.99);
    let (heat, again) = heat_release::heat_release_j(combustion, &profile, fraction, -6.0);
    assert_eq!(heat, 0.0, "a backwards jump must not replay the burn");
    assert_eq!(again, fraction);
}

// --- the chain, running in the solver ---------------------------------------

#[test]
fn the_running_engine_reports_a_plausible_combustion_chain() {
    let mut engine = Engine::builtin().expect("catalog builds");
    // Start unloaded: a real engine cannot crank against full load either. The
    // dynamometer harness in `dyno.rs` is what measures loaded operating points.
    engine
        .set_controls(Controls {
            pedal: 1.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
        })
        .expect("controls accepted");

    let mut snapshot = engine.snapshot();
    for tick in 0..90 {
        snapshot = engine.advance(4_000).expect("advance");
        if tick == 20 {
            engine
                .set_controls(Controls {
                    pedal: 1.0,
                    load_torque_nm: 1800.0,
                    starter: false,
                    ignition: true,
                })
                .expect("controls accepted");
        }
    }

    assert!(snapshot.rpm > 600.0, "the engine should be pulling");
    assert_eq!(
        snapshot.injection_variant, "amplified",
        "full fuelling should select the published amplified APCRS variant"
    );
    assert_eq!(snapshot.injection_pressure_pa, 210.0e6);
    let delay_deg = snapshot.ignition_delay_rad * 180.0 / PI;
    assert!(
        (0.5..8.0).contains(&delay_deg),
        "ignition delay {delay_deg:.2} deg is implausible"
    );
    assert!((0.0..0.5).contains(&snapshot.premixed_fraction));
    assert!(snapshot.peak_pressure_pa_cycle > 10.0e6);
    assert!(snapshot.peak_pressure_pa_cycle < 23.0e6);
}

#[test]
fn light_load_selects_the_standard_variant() {
    let mut engine = Engine::builtin().expect("catalog builds");
    engine
        .set_controls(Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
        })
        .expect("controls accepted");
    for tick in 0..90 {
        let snapshot = engine.advance(4_000).expect("advance");
        if tick == 12 || snapshot.rpm > 320.0 {
            engine
                .set_controls(Controls {
                    pedal: 0.0,
                    load_torque_nm: 0.0,
                    starter: false,
                    ignition: true,
                })
                .expect("controls accepted");
        }
    }
    let snapshot = engine.snapshot();
    assert_eq!(
        snapshot.injection_variant, "standard",
        "idle fuelling should use rail pressure without amplification"
    );
    assert_eq!(snapshot.injection_pressure_pa, 90.0e6);
    assert!(
        snapshot.premixed_fraction > 0.5,
        "idle combustion should be premixed dominated, got {}",
        snapshot.premixed_fraction
    );
}
