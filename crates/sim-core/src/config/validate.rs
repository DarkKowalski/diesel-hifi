//! Configuration validation.
//!
//! Nothing reaches the solver without passing through here. Validation covers
//! schema version, physical ranges, numerical finiteness, and the provenance
//! rules from README "Configuration and provenance".

use std::collections::BTreeSet;
use std::f64::consts::PI;

use super::paths::PARAMETER_PATHS;
use super::provenance::{ProvenanceReport, ProvenanceStatus};
use super::{EngineConfig, StructuralMode, SCHEMA_VERSION};
use crate::error::{Result, SimError};

/// Largest cylinder count the fixed-size hot state supports.
pub const MAX_CYLINDERS: usize = 12;

/// One four-stroke cycle in radians.
pub const CYCLE_RAD: f64 = 4.0 * PI;

/// Quantities computed once from validated geometry so the hot loop does no
/// redundant work.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedGeometry {
    pub crank_radius_m: f64,
    pub piston_area_m2: f64,
    pub displacement_per_cylinder_m3: f64,
    pub total_displacement_m3: f64,
    pub clearance_volume_m3: f64,
    pub max_volume_m3: f64,
    pub stroke_bore_ratio: f64,
    /// Cycle-angle offset per cylinder, indexed by 0-based cylinder index.
    pub phase_offsets_rad: Vec<f64>,
}

/// An [`EngineConfig`] that has passed validation. The solver accepts nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedConfig {
    config: EngineConfig,
    derived: DerivedGeometry,
}

impl ValidatedConfig {
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn derived(&self) -> &DerivedGeometry {
        &self.derived
    }

    pub fn id(&self) -> &str {
        &self.config.identity.id
    }

    /// Provenance view for the UI, with counts by classification.
    pub fn provenance_report(&self) -> ProvenanceReport {
        let entries = self.config.provenance.clone();
        ProvenanceReport {
            config_id: self.config.identity.id.clone(),
            display_name: self.config.identity.display_name.clone(),
            disclaimer: self.config.identity.disclaimer.clone(),
            sources: self.config.sources.clone(),
            published_count: entries.iter().filter(|e| e.is_published()).count(),
            derived_count: entries.iter().filter(|e| e.is_derived()).count(),
            calibrated_count: entries.iter().filter(|e| e.is_calibrated()).count(),
            entries,
        }
    }
}

fn require(cond: bool, message: impl Into<String>) -> Result<()> {
    if cond {
        Ok(())
    } else {
        Err(SimError::invalid_config(message))
    }
}

fn finite_positive(value: f64, path: &str) -> Result<()> {
    require(
        value.is_finite() && value > 0.0,
        format!("`{path}` must be finite and positive, got {value}"),
    )
}

fn finite(value: f64, path: &str) -> Result<()> {
    require(
        value.is_finite(),
        format!("`{path}` must be finite, got {value}"),
    )
}

/// Validate an engine configuration and precompute derived geometry.
pub fn validate(config: EngineConfig) -> Result<ValidatedConfig> {
    require(
        config.schema_version == SCHEMA_VERSION,
        format!(
            "unsupported schema_version {}, expected {SCHEMA_VERSION}",
            config.schema_version
        ),
    )?;

    validate_identity(&config)?;
    validate_finite(&config)?;
    let derived = validate_geometry(&config)?;
    validate_ranges(&config)?;
    validate_provenance(&config)?;

    Ok(ValidatedConfig { config, derived })
}

fn validate_identity(config: &EngineConfig) -> Result<()> {
    require(
        !config.identity.id.trim().is_empty(),
        "identity.id must not be empty",
    )?;
    require(
        !config.identity.display_name.trim().is_empty(),
        "identity.display_name must not be empty",
    )?;
    require(
        !config.identity.disclaimer.trim().is_empty(),
        "identity.disclaimer must not be empty; the model must never be presented as OEM data",
    )
}

/// Every scalar parameter must be finite before anything else is checked.
fn validate_finite(config: &EngineConfig) -> Result<()> {
    for path in PARAMETER_PATHS {
        if let Some(value) = config.value_at(path) {
            finite(value, path)?;
        }
    }
    Ok(())
}

fn validate_geometry(config: &EngineConfig) -> Result<DerivedGeometry> {
    let g = &config.geometry;

    require(
        g.cylinders >= 1 && g.cylinders <= MAX_CYLINDERS,
        format!(
            "geometry.cylinders must be 1..={MAX_CYLINDERS}, got {}",
            g.cylinders
        ),
    )?;
    finite_positive(g.bore_m, "geometry.bore_m")?;
    finite_positive(g.stroke_m, "geometry.stroke_m")?;
    finite_positive(g.connecting_rod_m, "geometry.connecting_rod_m")?;
    require(
        g.compression_ratio > 1.0,
        format!(
            "geometry.compression_ratio must exceed 1, got {}",
            g.compression_ratio
        ),
    )?;

    let crank_radius_m = g.stroke_m * 0.5;
    require(
        g.connecting_rod_m > crank_radius_m,
        "geometry.connecting_rod_m must exceed the crank radius (stroke/2)",
    )?;

    // The firing order must be a permutation of 1..=cylinders, so that phase
    // offsets are derived from data rather than hard-coded per engine.
    require(
        g.firing_order.len() == g.cylinders,
        format!(
            "geometry.firing_order must list {} cylinders, got {}",
            g.cylinders,
            g.firing_order.len()
        ),
    )?;
    let unique: BTreeSet<usize> = g.firing_order.iter().copied().collect();
    require(
        unique.len() == g.cylinders
            && unique.iter().copied().min() == Some(1)
            && unique.iter().copied().max() == Some(g.cylinders),
        "geometry.firing_order must be a permutation of 1..=cylinders",
    )?;

    let piston_area_m2 = PI * 0.25 * g.bore_m * g.bore_m;
    let displacement_per_cylinder_m3 = piston_area_m2 * g.stroke_m;
    let total_displacement_m3 = displacement_per_cylinder_m3 * g.cylinders as f64;
    let clearance_volume_m3 = displacement_per_cylinder_m3 / (g.compression_ratio - 1.0);
    let max_volume_m3 = clearance_volume_m3 + displacement_per_cylinder_m3;
    let stroke_bore_ratio = g.stroke_m / g.bore_m;

    // Cross-check the published displacement against the recomputed geometry.
    finite_positive(
        g.published_displacement_m3,
        "geometry.published_displacement_m3",
    )?;
    let delta_l = (total_displacement_m3 - g.published_displacement_m3).abs() * 1000.0;
    require(
        delta_l <= 0.05,
        format!(
            "recomputed displacement {:.4} L differs from published {:.4} L by {delta_l:.4} L (limit 0.05 L)",
            total_displacement_m3 * 1000.0,
            g.published_displacement_m3 * 1000.0
        ),
    )?;

    // Cylinder k fires at k * (cycle / cylinders), evenly spaced.
    let interval = CYCLE_RAD / g.cylinders as f64;
    let mut phase_offsets_rad = vec![0.0; g.cylinders];
    for (position, cylinder_number) in g.firing_order.iter().enumerate() {
        phase_offsets_rad[cylinder_number - 1] = position as f64 * interval;
    }

    Ok(DerivedGeometry {
        crank_radius_m,
        piston_area_m2,
        displacement_per_cylinder_m3,
        total_displacement_m3,
        clearance_volume_m3,
        max_volume_m3,
        stroke_bore_ratio,
        phase_offsets_rad,
    })
}

fn validate_ranges(config: &EngineConfig) -> Result<()> {
    let v = &config.valvetrain;
    require(
        v.intake_valves_per_cylinder >= 1 && v.exhaust_valves_per_cylinder >= 1,
        "each cylinder needs at least one intake and one exhaust valve",
    )?;
    require(
        v.intake_valve_close_rad > -CYCLE_RAD / 2.0 && v.intake_valve_close_rad < 0.0,
        "valvetrain.intake_valve_close_rad must fall in (-2*PI, 0) relative to firing TDC",
    )?;
    require(
        v.exhaust_valve_open_rad > 0.0 && v.exhaust_valve_open_rad < CYCLE_RAD / 2.0,
        "valvetrain.exhaust_valve_open_rad must fall in (0, 2*PI) relative to firing TDC",
    )?;
    finite_positive(
        v.exhaust_effective_area_m2,
        "valvetrain.exhaust_effective_area_m2",
    )?;
    require(
        v.exhaust_ramp_rad > 0.0 && v.exhaust_ramp_rad < CYCLE_RAD / 8.0,
        "valvetrain.exhaust_ramp_rad must be positive and short compared with the \
         exhaust window",
    )?;

    let i = &config.injection;
    finite_positive(i.rail_pressure_max_pa, "injection.rail_pressure_max_pa")?;
    require(
        i.amplified_pressure_max_pa >= i.rail_pressure_max_pa,
        "injection.amplified_pressure_max_pa must be at least the rail pressure",
    )?;
    finite_positive(
        i.fuel_lower_heating_value_j_per_kg,
        "injection.fuel_lower_heating_value_j_per_kg",
    )?;
    finite_positive(i.fuel_density_kg_m3, "injection.fuel_density_kg_m3")?;
    finite_positive(i.max_fuel_mg_per_cycle, "injection.max_fuel_mg_per_cycle")?;
    finite_positive(i.smoke_limit_afr, "injection.smoke_limit_afr")?;
    finite_positive(i.stoichiometric_afr, "injection.stoichiometric_afr")?;
    require(
        i.smoke_limit_afr > i.stoichiometric_afr,
        "injection.smoke_limit_afr must exceed the stoichiometric ratio; a diesel \
         smoke-limits well before it runs out of air",
    )?;
    require(
        i.nozzle_hole_count >= 1,
        "injection.nozzle_hole_count must be at least 1",
    )?;
    finite_positive(i.nozzle_hole_diameter_m, "injection.nozzle_hole_diameter_m")?;
    require(
        i.discharge_coefficient > 0.0 && i.discharge_coefficient <= 1.0,
        "injection.discharge_coefficient must fall in (0, 1]",
    )?;
    require(
        i.amplified_variant_min_fuel_mg >= 0.0,
        "injection.amplified_variant_min_fuel_mg must not be negative",
    )?;
    i.soi_schedule.validate("injection.soi_schedule")?;
    require(
        i.soi_schedule.min_value() > -PI && i.soi_schedule.max_value() < PI,
        "injection.soi_schedule values must fall in (-PI, PI) relative to firing TDC",
    )?;
    require(
        i.soi_load_retard_rad_per_mg >= 0.0 && i.soi_load_retard_rad_per_mg < 0.01,
        "injection.soi_load_retard_rad_per_mg must fall in [0, 0.01)",
    )?;

    let c = &config.combustion;
    require(
        c.combustion_efficiency > 0.0 && c.combustion_efficiency <= 1.0,
        "combustion.combustion_efficiency must fall in (0, 1]",
    )?;
    require(
        c.cetane_number > 0.0 && c.cetane_number < 100.0,
        "combustion.cetane_number must fall in (0, 100)",
    )?;
    finite_positive(c.premixed_wiebe_shape, "combustion.premixed_wiebe_shape")?;
    finite_positive(
        c.premixed_wiebe_efficiency,
        "combustion.premixed_wiebe_efficiency",
    )?;
    finite_positive(c.premixed_duration_rad, "combustion.premixed_duration_rad")?;
    finite_positive(c.diffusion_wiebe_shape, "combustion.diffusion_wiebe_shape")?;
    finite_positive(
        c.diffusion_wiebe_efficiency,
        "combustion.diffusion_wiebe_efficiency",
    )?;
    finite_positive(
        c.diffusion_duration_rad_per_mg,
        "combustion.diffusion_duration_rad_per_mg",
    )?;
    finite_positive(
        c.diffusion_duration_min_rad,
        "combustion.diffusion_duration_min_rad",
    )?;
    require(
        c.max_premixed_fraction > 0.0 && c.max_premixed_fraction <= 1.0,
        "combustion.max_premixed_fraction must fall in (0, 1]",
    )?;

    let ht = &config.heat_transfer;
    require(
        ht.woschni_c1_closed > 0.0 && ht.woschni_c1_gas_exchange > 0.0,
        "Woschni C1 coefficients must be positive",
    )?;
    require(
        ht.woschni_c2 >= 0.0,
        "heat_transfer.woschni_c2 must not be negative",
    )?;
    finite_positive(ht.wall_temperature_k, "heat_transfer.wall_temperature_k")?;

    let gas = &config.gas;
    finite_positive(gas.gas_constant_j_per_kg_k, "gas.gas_constant_j_per_kg_k")?;
    finite_positive(gas.cv_reference_j_per_kg_k, "gas.cv_reference_j_per_kg_k")?;
    require(
        gas.cv_slope_j_per_kg_k2 >= 0.0,
        "gas.cv_slope_j_per_kg_k2 must not be negative",
    )?;
    finite_positive(gas.reference_temperature_k, "gas.reference_temperature_k")?;

    let a = &config.air_path;
    finite_positive(a.ambient_pressure_pa, "air_path.ambient_pressure_pa")?;
    finite_positive(a.ambient_temperature_k, "air_path.ambient_temperature_k")?;
    a.boost_target_schedule
        .validate("air_path.boost_target_schedule")?;
    require(
        a.boost_target_schedule.min_value() >= a.ambient_pressure_pa,
        "air_path.boost_target_schedule must never fall below ambient pressure",
    )?;
    // Manifold volumes set the stiffness of the filling dynamics. Too small and
    // the pressure states move faster than the fixed step can follow.
    require(
        a.intake_manifold_volume_m3 >= 1.0e-3 && a.intake_manifold_volume_m3 <= 1.0,
        "air_path.intake_manifold_volume_m3 must fall in [1e-3, 1] m^3; smaller volumes \
         make the filling dynamics too stiff for the fixed step",
    )?;
    require(
        a.exhaust_manifold_volume_m3 >= 1.0e-3 && a.exhaust_manifold_volume_m3 <= 1.0,
        "air_path.exhaust_manifold_volume_m3 must fall in [1e-3, 1] m^3; smaller volumes \
         make the filling dynamics too stiff for the fixed step",
    )?;
    require(
        a.intercooler_effectiveness >= 0.0 && a.intercooler_effectiveness <= 1.0,
        "air_path.intercooler_effectiveness must fall in [0, 1]",
    )?;
    finite_positive(a.coolant_temperature_k, "air_path.coolant_temperature_k")?;
    require(
        a.exhaust_restriction_pa_per_kg2_s2 >= 0.0,
        "air_path.exhaust_restriction_pa_per_kg2_s2 must not be negative",
    )?;
    finite_positive(a.crankcase_pressure_pa, "air_path.crankcase_pressure_pa")?;
    require(
        a.volumetric_efficiency > 0.0 && a.volumetric_efficiency <= 1.5,
        "air_path.volumetric_efficiency must fall in (0, 1.5]",
    )?;

    let t = &config.turbo;
    finite_positive(t.shaft_inertia_kg_m2, "turbo.shaft_inertia_kg_m2")?;
    finite_positive(
        t.compressor_wheel_diameter_m,
        "turbo.compressor_wheel_diameter_m",
    )?;
    require(
        t.compressor_efficiency > 0.0 && t.compressor_efficiency <= 1.0,
        "turbo.compressor_efficiency must fall in (0, 1]",
    )?;
    require(
        t.turbine_efficiency > 0.0 && t.turbine_efficiency <= 1.0,
        "turbo.turbine_efficiency must fall in (0, 1]",
    )?;
    finite_positive(
        t.compressor_head_coefficient,
        "turbo.compressor_head_coefficient",
    )?;
    finite_positive(
        t.compressor_max_flow_kg_s_per_rad_s,
        "turbo.compressor_max_flow_kg_s_per_rad_s",
    )?;
    finite_positive(
        t.turbine_effective_area_m2,
        "turbo.turbine_effective_area_m2",
    )?;
    require(
        t.bearing_friction_nm_per_rad_s >= 0.0,
        "turbo.bearing_friction_nm_per_rad_s must not be negative",
    )?;
    finite_positive(
        t.max_shaft_speed_rad_per_s,
        "turbo.max_shaft_speed_rad_per_s",
    )?;
    finite_positive(t.wastegate_max_area_m2, "turbo.wastegate_max_area_m2")?;
    require(
        t.wastegate_p_gain_per_pa >= 0.0 && t.wastegate_i_gain_per_pa_s >= 0.0,
        "wastegate controller gains must not be negative",
    )?;
    finite_positive(t.wastegate_slew_per_s, "turbo.wastegate_slew_per_s")?;

    let e = &config.egr;
    finite_positive(
        e.cooler_inlet_temperature_k,
        "egr.cooler_inlet_temperature_k",
    )?;
    require(
        e.cooler_outlet_temperature_k > 0.0
            && e.cooler_outlet_temperature_k < e.cooler_inlet_temperature_k,
        "egr.cooler_outlet_temperature_k must be positive and below the inlet temperature",
    )?;
    require(
        e.cooler_effectiveness >= 0.0 && e.cooler_effectiveness <= 1.0,
        "egr.cooler_effectiveness must fall in [0, 1]",
    )?;
    e.rate_schedule.validate("egr.rate_schedule")?;
    require(
        e.rate_schedule.min_value() >= 0.0 && e.rate_schedule.max_value() <= e.max_rate,
        "egr.rate_schedule values must fall in [0, egr.max_rate]",
    )?;
    finite_positive(e.valve_max_area_m2, "egr.valve_max_area_m2")?;
    require(
        e.valve_discharge_coefficient > 0.0 && e.valve_discharge_coefficient <= 1.0,
        "egr.valve_discharge_coefficient must fall in (0, 1]",
    )?;
    require(
        e.rate_p_gain >= 0.0 && e.rate_i_gain_per_s >= 0.0,
        "EGR rate controller gains must not be negative",
    )?;
    // Above roughly half the charge being recirculated the engine cannot burn
    // cleanly at all; the manual notes soot, CO and HC rise when the exhaust
    // proportion is too high.
    require(
        e.max_rate > 0.0 && e.max_rate <= 0.5,
        "egr.max_rate must fall in (0, 0.5]",
    )?;

    let b = &config.engine_brake;
    require(
        !b.variant.trim().is_empty(),
        "engine_brake.variant must name the fitted code, such as M5U",
    )?;
    finite_positive(b.min_speed_rpm, "engine_brake.min_speed_rpm")?;
    require(
        b.stage1_cylinder_count > 0 && b.stage1_cylinder_count <= config.geometry.cylinders as u32,
        "engine_brake.stage1_cylinder_count must name at least one and at most every cylinder",
    )?;
    finite_positive(b.anchor_low_rpm, "engine_brake.anchor_low_rpm")?;
    finite_positive(b.anchor_low_power_w, "engine_brake.anchor_low_power_w")?;
    finite_positive(b.anchor_high_rpm, "engine_brake.anchor_high_rpm")?;
    finite_positive(b.anchor_high_power_w, "engine_brake.anchor_high_power_w")?;
    require(
        b.anchor_high_rpm > b.anchor_low_rpm && b.anchor_high_power_w > b.anchor_low_power_w,
        "the engine brake anchors must be ordered: the upper one is a higher speed and more power",
    )?;
    // Both lobes have to fall inside the closed period, or they are not a
    // decompression brake at all: the charging lobe must come after the intake
    // valve shuts, and the release lobe must come before the exhaust valve opens
    // on its own. A lobe outside that window would silently do nothing.
    let vt = &config.valvetrain;
    finite_positive(b.charge_width_rad, "engine_brake.charge_width_rad")?;
    finite_positive(b.release_width_rad, "engine_brake.release_width_rad")?;
    require(
        b.charge_center_rad - b.charge_width_rad > vt.intake_valve_close_rad,
        "engine_brake.charge_center_rad must open after the intake valve has closed",
    )?;
    require(
        b.release_center_rad + b.release_width_rad < vt.exhaust_valve_open_rad,
        "engine_brake.release_center_rad must close before the exhaust valve opens",
    )?;
    require(
        b.charge_center_rad + b.charge_width_rad < b.release_center_rad - b.release_width_rad,
        "the charging and release lobes must not overlap",
    )?;
    require(
        b.effective_area_m2 > 0.0 && b.effective_area_m2 <= vt.exhaust_effective_area_m2,
        "engine_brake.effective_area_m2 must be positive and no larger than the full exhaust port",
    )?;
    for (value, path) in [
        (
            b.stage1_boost_target_pa,
            "engine_brake.stage1_boost_target_pa",
        ),
        (
            b.stage2_boost_target_pa,
            "engine_brake.stage2_boost_target_pa",
        ),
        (
            b.stage3_boost_target_pa,
            "engine_brake.stage3_boost_target_pa",
        ),
    ] {
        finite_positive(value, path)?;
    }
    require(
        b.wastegate_gain_scale > 0.0 && b.wastegate_gain_scale <= 1.0,
        "engine_brake.wastegate_gain_scale must fall in (0, 1]",
    )?;
    for (value, path) in [
        (b.stage1_egr_command, "engine_brake.stage1_egr_command"),
        (b.stage2_egr_command, "engine_brake.stage2_egr_command"),
        (b.stage3_egr_command, "engine_brake.stage3_egr_command"),
    ] {
        require(
            (0.0..=1.0).contains(&value),
            format!("{path} must fall in [0, 1]"),
        )?;
    }

    let d = &config.driveline;
    finite_positive(d.vehicle_mass_kg, "driveline.vehicle_mass_kg")?;
    finite_positive(d.wheel_radius_m, "driveline.wheel_radius_m")?;
    require(
        d.rolling_resistance_coeff >= 0.0 && d.rolling_resistance_coeff <= 0.1,
        "driveline.rolling_resistance_coeff must fall in [0, 0.1]",
    )?;
    require(
        d.drag_area_m2 >= 0.0 && d.drag_area_m2.is_finite(),
        "driveline.drag_area_m2 must not be negative",
    )?;
    finite_positive(d.final_drive_ratio, "driveline.final_drive_ratio")?;
    require(
        !d.gear_ratios.is_empty(),
        "driveline.gear_ratios must list at least one forward gear",
    )?;
    for (index, ratio) in d.gear_ratios.iter().enumerate() {
        finite_positive(*ratio, "driveline.gear_ratios")?;
        // Lowest gear first, so the ratios fall monotonically. A gearbox listed
        // out of order would make the gear selector nonsense without failing any
        // other check.
        if index > 0 {
            require(
                *ratio < d.gear_ratios[index - 1],
                "driveline.gear_ratios must be listed lowest gear first, strictly decreasing",
            )?;
        }
    }
    require(
        d.driveline_efficiency > 0.0 && d.driveline_efficiency <= 1.0,
        "driveline.driveline_efficiency must fall in (0, 1]",
    )?;
    require(
        d.max_grade_percent > 0.0 && d.max_grade_percent <= 100.0,
        "driveline.max_grade_percent must fall in (0, 100]",
    )?;

    // The solver step is checked before audio, because the audio cutoffs are
    // validated against the sample rate it implies. Reporting "the low-pass is
    // above Nyquist" when the real fault is an absurd step size would point at
    // the wrong field.
    let s = &config.solver;
    require(
        s.fixed_step_s > 0.0 && s.fixed_step_s <= 1.0e-3,
        "solver.fixed_step_s must fall in (0, 1e-3] seconds",
    )?;

    let ex = &config.exhaust_system;
    finite_positive(ex.tailpipe_length_m, "exhaust_system.tailpipe_length_m")?;
    finite_positive(ex.duct_area_m2, "exhaust_system.duct_area_m2")?;
    finite_positive(ex.radiation_cutoff_hz, "exhaust_system.radiation_cutoff_hz")?;
    finite_positive(
        ex.turbine_loss_cutoff_hz,
        "exhaust_system.turbine_loss_cutoff_hz",
    )?;
    require(
        ex.aftertreatment_volume_m3.is_finite() && ex.aftertreatment_volume_m3 >= 0.0,
        "exhaust_system.aftertreatment_volume_m3 must be finite and not negative",
    )?;
    // The substrate loss is the other half of the box, and it is a transmission
    // rather than a gain: above one it would add energy on every traverse, which
    // is a duct that plays itself. Zero is allowed and means a box that stops the
    // wave dead - useless, but not unstable.
    require(
        ex.aftertreatment_transmission.is_finite()
            && ex.aftertreatment_transmission >= 0.0
            && ex.aftertreatment_transmission <= 1.0,
        "exhaust_system.aftertreatment_transmission must fall in [0, 1]: a substrate \
         attenuates the wave passing through it and cannot amplify it",
    )?;
    require(
        ex.turbine_insertion_loss_db.is_finite() && ex.turbine_insertion_loss_db >= 0.0,
        "exhaust_system.turbine_insertion_loss_db must be finite and not negative",
    )?;
    // Duct stability. The tip reflection is half of the waveguide's loop gain -
    // the manifold end supplies the rest - so a magnitude at or above unity is a
    // loop that grows without bound in the hot loop, where nothing can recover
    // it. An open end is a pressure node, so the sign must be negative as well.
    require(
        ex.open_end_reflection.is_finite()
            && ex.open_end_reflection < 0.0
            && ex.open_end_reflection > -1.0,
        "exhaust_system.open_end_reflection must fall in (-1, 0): an open end inverts the \
         wave, and a magnitude of one or more makes the duct unstable",
    )?;
    finite_positive(ex.runner_length_min_m, "exhaust_system.runner_length_min_m")?;
    require(
        ex.runner_length_max_m > ex.runner_length_min_m,
        "exhaust_system.runner_length_max_m must exceed runner_length_min_m; a manifold \
         whose runners are all the same length cannot make the firing order audible",
    )?;
    // Both duct buffers are sized from these at construction, so an absurd
    // length is a memory question as much as a physical one.
    require(
        ex.tailpipe_length_m <= 20.0 && ex.runner_length_max_m <= 5.0,
        "exhaust_system lengths must stay within 20 m of duct and 5 m of runner",
    )?;

    let au = &config.audio;
    require(
        au.exhaust_gain >= 0.0,
        "audio.exhaust_gain must not be negative",
    )?;
    require(
        au.highpass_cutoff_hz > 0.0 && au.highpass_cutoff_hz < 0.5 / config.solver.fixed_step_s,
        "audio.highpass_cutoff_hz must be positive and below the Nyquist frequency of the \
         solver step",
    )?;
    // The knee is also the ceiling the soft clipper saturates at, so keeping it
    // at or below unity is what guarantees samples stay inside [-1, 1].
    require(
        au.soft_clip_knee > 0.0 && au.soft_clip_knee <= 1.0,
        "audio.soft_clip_knee must fall in (0, 1]",
    )?;
    require(
        au.structural_gain.is_finite() && au.structural_gain >= 0.0,
        "audio.structural_gain must be finite and not negative",
    )?;
    require(
        au.body_gain.is_finite() && au.body_gain >= 0.0,
        "audio.body_gain must be finite and not negative",
    )?;

    // Build scatter. Zero is legal and means a perfect engine; the upper bound
    // is what keeps a "realism" knob from quietly becoming a calibration
    // change, since a spread wide enough to move peak power is a spread that is
    // no longer modelling manufacturing tolerance.
    let vt_spread = config.valvetrain.exhaust_area_spread;
    require(
        vt_spread.is_finite() && (0.0..=0.1).contains(&vt_spread),
        "valvetrain.exhaust_area_spread must be finite and fall in [0, 0.1]",
    )?;
    let inj_spread = config.injection.cylinder_delivery_spread;
    require(
        inj_spread.is_finite() && (0.0..=0.1).contains(&inj_spread),
        "injection.cylinder_delivery_spread must be finite and fall in [0, 0.1]",
    )?;
    // The per-cycle sibling, held to the same bound for the same reason.
    let cycle_spread = config.injection.cycle_delivery_spread;
    require(
        cycle_spread.is_finite() && (0.0..=0.1).contains(&cycle_spread),
        "injection.cycle_delivery_spread must be finite and fall in [0, 0.1]",
    )?;

    // Both modal banks, checked identically because they are the same filter
    // structure driven by different quantities. Sharing the check rather than
    // duplicating it means a bank cannot be added later that skips the stability
    // condition, which is the one that matters: an unstable pole grows without
    // bound in the hot loop where nothing can recover it.
    let nyquist_hz = 0.5 / config.solver.fixed_step_s;
    let check_modes = |modes: &[StructuralMode], name: &str| -> crate::error::Result<()> {
        require(
            !modes.is_empty(),
            format!("audio.{name} must name at least one mode"),
        )?;
        for (index, mode) in modes.iter().enumerate() {
            finite_positive(
                mode.frequency_hz,
                &format!("audio.{name}[{index}].frequency_hz"),
            )?;
            finite_positive(mode.q, &format!("audio.{name}[{index}].q"))?;
            require(
                mode.gain.is_finite() && mode.gain >= 0.0,
                format!("audio.{name} gains must be finite and not negative"),
            )?;
            require(
                mode.frequency_hz < nyquist_hz,
                format!(
                    "audio.{name} frequencies must stay below the Nyquist \
                     frequency of the solver step"
                ),
            )?;
            // Stability of the two-pole resonator, which is the whole reason
            // this check exists rather than a bare range on `q`. The pole radius
            // is `r = 1 - theta / (2 Q)` with `theta = 2 pi f dt`; the filter is
            // stable only for `0 < r < 1`. A high frequency paired with a low Q
            // drives `r` negative or past zero and the mode would grow without
            // bound in the hot loop, where nothing can recover it. Reject it
            // here instead.
            let theta = std::f64::consts::TAU * mode.frequency_hz * config.solver.fixed_step_s;
            let r = 1.0 - theta / (2.0 * mode.q);
            require(
                r > 0.0 && r < 1.0,
                format!(
                    "audio.{name} must give a stable resonator: the pole radius \
                     1 - pi f dt / Q must fall in (0, 1), so a high frequency needs a \
                     correspondingly high Q"
                ),
            )?;
        }
        Ok(())
    };
    check_modes(&au.structural_modes, "structural_modes")?;
    check_modes(&au.body_modes, "body_modes")?;

    let gov = &config.governor;
    finite_positive(gov.idle_target_rpm, "governor.idle_target_rpm")?;
    require(
        gov.idle_p_gain_mg_per_rad_s >= 0.0 && gov.idle_i_gain_mg_per_rad >= 0.0,
        "governor gains must not be negative",
    )?;
    finite_positive(
        gov.idle_integral_limit_mg,
        "governor.idle_integral_limit_mg",
    )?;
    require(
        gov.overspeed_taper_start_rpm > gov.idle_target_rpm,
        "governor.overspeed_taper_start_rpm must exceed the idle target",
    )?;
    require(
        gov.overspeed_cutoff_rpm > gov.overspeed_taper_start_rpm,
        "governor.overspeed_cutoff_rpm must exceed the taper start",
    )?;

    let l = &config.load;
    require(
        l.accessory_torque_constant_nm >= 0.0 && l.accessory_torque_per_rad_s >= 0.0,
        "accessory torque terms must not be negative",
    )?;
    finite_positive(l.max_external_load_nm, "load.max_external_load_nm")?;
    finite_positive(l.starter_torque_nm, "load.starter_torque_nm")?;
    finite_positive(l.starter_cutout_rpm, "load.starter_cutout_rpm")?;

    finite_positive(
        config.inertia.rotating_inertia_kg_m2,
        "inertia.rotating_inertia_kg_m2",
    )?;
    finite_positive(
        config.inertia.complete_engine_mass_kg,
        "inertia.complete_engine_mass_kg",
    )?;

    let lim = &config.limits;
    finite_positive(
        lim.max_combustion_pressure_pa,
        "limits.max_combustion_pressure_pa",
    )?;
    require(
        lim.max_combustion_pressure_pa > config.air_path.ambient_pressure_pa,
        "limits.max_combustion_pressure_pa must exceed the exhaust manifold pressure",
    )?;
    require(
        lim.max_rpm > gov.overspeed_cutoff_rpm,
        "limits.max_rpm must exceed governor.overspeed_cutoff_rpm",
    )?;
    finite_positive(lim.max_gas_temperature_k, "limits.max_gas_temperature_k")?;

    let s = &config.solver;
    require(
        s.fixed_step_s > 0.0 && s.fixed_step_s <= 1.0e-3,
        "solver.fixed_step_s must fall in (0, 1e-3] seconds",
    )?;
    require(
        s.max_steps_per_batch >= 1,
        "solver.max_steps_per_batch must be at least 1",
    )?;

    finite_positive(config.rated.max_power_w, "rated.max_power_w")?;
    finite_positive(config.rated.max_torque_nm, "rated.max_torque_nm")?;

    require(
        !config.sources.is_empty(),
        "at least one source record is required",
    )
}

fn validate_provenance(config: &EngineConfig) -> Result<()> {
    let expected: BTreeSet<&str> = PARAMETER_PATHS.iter().copied().collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let source_ids: BTreeSet<&str> = config.sources.iter().map(|s| s.id.as_str()).collect();

    for entry in &config.provenance {
        let path = entry.path.as_str();
        require(
            expected.contains(path),
            format!("provenance entry names unknown parameter path `{path}`"),
        )?;
        require(
            seen.insert(path),
            format!("duplicate provenance entry for `{path}`"),
        )?;
        require(
            !entry.value_note.trim().is_empty(),
            format!("`{path}` provenance needs a value_note"),
        )?;

        match entry.status {
            ProvenanceStatus::Published => {
                let source_id = entry.source_id.as_deref().unwrap_or("").trim();
                require(
                    !source_id.is_empty(),
                    format!("published `{path}` must name a source_id"),
                )?;
                require(
                    source_ids.contains(source_id),
                    format!("published `{path}` names unknown source_id `{source_id}`"),
                )?;
                require(
                    !entry.locator.as_deref().unwrap_or("").trim().is_empty(),
                    format!("published `{path}` must give a locator (document, section, or page)"),
                )?;
            }
            ProvenanceStatus::Derived => {
                require(
                    !entry.formula.as_deref().unwrap_or("").trim().is_empty(),
                    format!("derived `{path}` must state its formula"),
                )?;
                require(
                    !entry.inputs.is_empty(),
                    format!("derived `{path}` must list its inputs"),
                )?;
            }
            ProvenanceStatus::Calibrated => {
                require(
                    !entry.purpose.as_deref().unwrap_or("").trim().is_empty(),
                    format!("calibrated `{path}` must state its purpose"),
                )?;
                let range = entry.safe_range.ok_or_else(|| {
                    SimError::invalid_config(format!("calibrated `{path}` must give a safe_range"))
                })?;
                require(
                    range[0].is_finite() && range[1].is_finite() && range[0] <= range[1],
                    format!("calibrated `{path}` has a malformed safe_range"),
                )?;
                if let Some(value) = config.value_at(path) {
                    require(
                        value >= range[0] && value <= range[1],
                        format!(
                            "calibrated `{path}` value {value} lies outside its safe_range [{}, {}]",
                            range[0], range[1]
                        ),
                    )?;
                }
            }
        }
    }

    let missing: Vec<&str> = expected.difference(&seen).copied().collect();
    require(
        missing.is_empty(),
        format!("missing provenance for: {}", missing.join(", ")),
    )
}
