//! Configuration validation.
//!
//! Nothing reaches the solver without passing through here. Validation covers
//! schema version, physical ranges, numerical finiteness, and the provenance
//! rules from SPEC section 5.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use super::paths::PARAMETER_PATHS;
use super::provenance::{ProvenanceReport, ProvenanceStatus};
use super::{EngineConfig, SCHEMA_VERSION};
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
    finite_positive(i.max_fuel_mg_per_cycle, "injection.max_fuel_mg_per_cycle")?;
    finite_positive(i.smoke_limit_afr, "injection.smoke_limit_afr")?;

    let c = &config.combustion;
    require(
        c.start_of_combustion_rad > -PI && c.start_of_combustion_rad < PI,
        "combustion.start_of_combustion_rad must fall in (-PI, PI) relative to firing TDC",
    )?;
    finite_positive(c.burn_duration_rad, "combustion.burn_duration_rad")?;
    require(
        c.combustion_efficiency > 0.0 && c.combustion_efficiency <= 1.0,
        "combustion.combustion_efficiency must fall in (0, 1]",
    )?;
    finite_positive(
        c.burned_gas_cv_j_per_kg_k,
        "combustion.burned_gas_cv_j_per_kg_k",
    )?;
    require(
        c.polytropic_compression > 1.0 && c.polytropic_compression < 2.0,
        "combustion.polytropic_compression must fall in (1, 2)",
    )?;
    require(
        c.polytropic_expansion > 1.0 && c.polytropic_expansion < 2.0,
        "combustion.polytropic_expansion must fall in (1, 2)",
    )?;

    let a = &config.air_path;
    finite_positive(
        a.intake_manifold_pressure_pa,
        "air_path.intake_manifold_pressure_pa",
    )?;
    finite_positive(
        a.intake_manifold_temperature_k,
        "air_path.intake_manifold_temperature_k",
    )?;
    finite_positive(
        a.exhaust_manifold_pressure_pa,
        "air_path.exhaust_manifold_pressure_pa",
    )?;
    finite_positive(a.crankcase_pressure_pa, "air_path.crankcase_pressure_pa")?;
    finite_positive(
        a.gas_constant_j_per_kg_k,
        "air_path.gas_constant_j_per_kg_k",
    )?;
    require(
        a.volumetric_efficiency > 0.0 && a.volumetric_efficiency <= 1.5,
        "air_path.volumetric_efficiency must fall in (0, 1.5]",
    )?;

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
        lim.max_combustion_pressure_pa > config.air_path.exhaust_manifold_pressure_pa,
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
