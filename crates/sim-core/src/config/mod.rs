//! Versioned, validated engine configuration.
//!
//! All simulation inputs live here. The solver reads only [`ValidatedConfig`];
//! it never embeds engine-specific calibration and never branches on an engine
//! ID (SPEC section 5).
//!
//! Sections carry `#[serde(default)]` so that Milestones 2-4 can add turbo, EGR,
//! aftertreatment, and engine-brake fields without revving `schema_version` or
//! invalidating existing configuration documents.

pub mod paths;
pub mod provenance;
pub mod validate;

use serde::{Deserialize, Serialize};

pub use provenance::{ProvenanceEntry, ProvenanceReport, ProvenanceStatus, SourceRecord};
pub use validate::ValidatedConfig;

/// The only configuration schema version understood by this build.
pub const SCHEMA_VERSION: u32 = 1;

/// Identity and display metadata. Manufacturer names are factual references only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Stable selection key.
    pub id: String,
    pub display_name: String,
    /// Factual reference to the real engine. Not an endorsement or a trademark claim.
    pub manufacturer_reference: String,
    /// Power code the configuration represents, e.g. `M3D`.
    pub power_code: String,
    /// Shown in the UI so the model is never mistaken for an OEM calibration.
    pub disclaimer: String,
}

/// Cylinder and crank geometry. SI units, radians.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub cylinders: usize,
    pub bore_m: f64,
    pub stroke_m: f64,
    pub connecting_rod_m: f64,
    pub compression_ratio: f64,
    /// 1-based cylinder numbers in firing sequence. Phase offsets are derived
    /// from this, so no engine-specific offsets are hard-coded in the solver.
    pub firing_order: Vec<usize>,
    /// Published displacement, retained for the cross-check against geometry.
    pub published_displacement_m3: f64,
    /// Published stroke:bore ratio, retained for the derived cross-check.
    pub published_stroke_bore_ratio: f64,
}

/// Valve counts (published) and the simplified gas-exchange boundaries used in
/// Milestone 1. Real valve events are not published in the source manual.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Valvetrain {
    pub intake_valves_per_cylinder: u32,
    pub exhaust_valves_per_cylinder: u32,
    /// Signed crank angle from firing TDC at which the cylinder becomes closed.
    /// Negative is before firing TDC (during the compression stroke).
    pub intake_valve_close_rad: f64,
    /// Signed crank angle from firing TDC at which the cylinder reopens.
    pub exhaust_valve_open_rad: f64,
}

/// Injection limits and fuel properties.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Injection {
    pub rail_pressure_max_pa: f64,
    pub amplified_pressure_max_pa: f64,
    pub fuel_lower_heating_value_j_per_kg: f64,
    pub max_fuel_mg_per_cycle: f64,
    /// Minimum air/fuel ratio permitted before fuelling is clipped (smoke limit).
    pub smoke_limit_afr: f64,
}

/// **Milestone 1 placeholder heat release.** A single smooth burn bump with no
/// ignition delay and no premixed/diffusion split. Milestone 2 replaces this
/// section and its provenance entries wholesale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Combustion {
    /// Signed crank angle from firing TDC where heat release begins.
    pub start_of_combustion_rad: f64,
    pub burn_duration_rad: f64,
    pub combustion_efficiency: f64,
    /// Effective constant-volume specific heat used for the heat-addition term.
    pub burned_gas_cv_j_per_kg_k: f64,
    pub polytropic_compression: f64,
    pub polytropic_expansion: f64,
}

/// Chen-Flynn style friction mean effective pressure terms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Friction {
    pub fmep_constant_pa: f64,
    pub fmep_peak_pressure_coeff: f64,
    pub fmep_piston_speed_coeff_pa_s_per_m: f64,
    pub fmep_piston_speed_sq_coeff: f64,
}

/// Air path. Milestone 1 holds the manifolds at fixed conditions; the turbo and
/// EGR fields arrive in Milestone 3 without changing this section's shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AirPath {
    pub intake_manifold_pressure_pa: f64,
    pub intake_manifold_temperature_k: f64,
    pub exhaust_manifold_pressure_pa: f64,
    /// Pressure on the underside of the piston, used for the net gas force.
    pub crankcase_pressure_pa: f64,
    pub gas_constant_j_per_kg_k: f64,
    pub volumetric_efficiency: f64,
}

/// Idle governor and overspeed limiting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Governor {
    pub idle_target_rpm: f64,
    pub idle_p_gain_mg_per_rad_s: f64,
    pub idle_i_gain_mg_per_rad: f64,
    pub idle_integral_limit_mg: f64,
    pub overspeed_taper_start_rpm: f64,
    pub overspeed_cutoff_rpm: f64,
}

/// Accessory, external load, and starter torque.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub accessory_torque_constant_nm: f64,
    pub accessory_torque_per_rad_s: f64,
    pub max_external_load_nm: f64,
    pub starter_torque_nm: f64,
    pub starter_cutout_rpm: f64,
}

/// Rotating inertia. The published complete-engine mass is metadata only and is
/// never used as flywheel or rotating inertia (SPEC section 4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Inertia {
    pub rotating_inertia_kg_m2: f64,
    pub complete_engine_mass_kg: f64,
}

/// Physical limits and safe numerical ranges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Validation/safety envelope, not a normal pressure target.
    pub max_combustion_pressure_pa: f64,
    pub max_rpm: f64,
    pub max_gas_temperature_k: f64,
}

/// Fixed-step integration parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Solver {
    pub fixed_step_s: f64,
    pub max_steps_per_batch: u32,
}

/// Reserved for Milestone 3. Unused by the Milestone 1 solver.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioCalibration {
    pub reference_spl_db: f64,
    pub exhaust_gain: f64,
}

/// Published rated output. Milestone 1 makes **no** calibration claim against
/// these figures, and the manual does not publish the engine speeds at which
/// they occur.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RatedOutput {
    pub max_power_w: f64,
    pub max_power_hp: f64,
    pub max_torque_nm: f64,
}

/// The complete, versioned engine configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineConfig {
    pub schema_version: u32,
    pub identity: Identity,
    pub geometry: Geometry,
    pub valvetrain: Valvetrain,
    pub injection: Injection,
    pub combustion: Combustion,
    pub friction: Friction,
    pub air_path: AirPath,
    pub governor: Governor,
    pub load: Load,
    pub inertia: Inertia,
    pub limits: Limits,
    pub solver: Solver,
    pub audio: AudioCalibration,
    pub rated: RatedOutput,
    pub sources: Vec<SourceRecord>,
    pub provenance: Vec<ProvenanceEntry>,
}

impl EngineConfig {
    /// Parse a configuration document from JSON.
    pub fn from_json(text: &str) -> crate::error::Result<Self> {
        serde_json::from_str(text)
            .map_err(|e| crate::error::SimError::invalid_config(format!("malformed config: {e}")))
    }

    /// Validate ranges and provenance, producing a [`ValidatedConfig`].
    pub fn validate(self) -> crate::error::Result<ValidatedConfig> {
        validate::validate(self)
    }

    /// Scalar value at a dotted parameter path, if that path names a number.
    ///
    /// Returns `None` for known non-scalar paths (such as `geometry.firing_order`)
    /// and for unknown paths.
    pub fn value_at(&self, path: &str) -> Option<f64> {
        let v = match path {
            "geometry.cylinders" => self.geometry.cylinders as f64,
            "geometry.bore_m" => self.geometry.bore_m,
            "geometry.stroke_m" => self.geometry.stroke_m,
            "geometry.connecting_rod_m" => self.geometry.connecting_rod_m,
            "geometry.compression_ratio" => self.geometry.compression_ratio,
            "geometry.firing_order" => return None,
            "geometry.published_displacement_m3" => self.geometry.published_displacement_m3,
            "geometry.published_stroke_bore_ratio" => self.geometry.published_stroke_bore_ratio,

            "valvetrain.intake_valves_per_cylinder" => {
                f64::from(self.valvetrain.intake_valves_per_cylinder)
            }
            "valvetrain.exhaust_valves_per_cylinder" => {
                f64::from(self.valvetrain.exhaust_valves_per_cylinder)
            }
            "valvetrain.intake_valve_close_rad" => self.valvetrain.intake_valve_close_rad,
            "valvetrain.exhaust_valve_open_rad" => self.valvetrain.exhaust_valve_open_rad,

            "injection.rail_pressure_max_pa" => self.injection.rail_pressure_max_pa,
            "injection.amplified_pressure_max_pa" => self.injection.amplified_pressure_max_pa,
            "injection.fuel_lower_heating_value_j_per_kg" => {
                self.injection.fuel_lower_heating_value_j_per_kg
            }
            "injection.max_fuel_mg_per_cycle" => self.injection.max_fuel_mg_per_cycle,
            "injection.smoke_limit_afr" => self.injection.smoke_limit_afr,

            "combustion.start_of_combustion_rad" => self.combustion.start_of_combustion_rad,
            "combustion.burn_duration_rad" => self.combustion.burn_duration_rad,
            "combustion.combustion_efficiency" => self.combustion.combustion_efficiency,
            "combustion.burned_gas_cv_j_per_kg_k" => self.combustion.burned_gas_cv_j_per_kg_k,
            "combustion.polytropic_compression" => self.combustion.polytropic_compression,
            "combustion.polytropic_expansion" => self.combustion.polytropic_expansion,

            "friction.fmep_constant_pa" => self.friction.fmep_constant_pa,
            "friction.fmep_peak_pressure_coeff" => self.friction.fmep_peak_pressure_coeff,
            "friction.fmep_piston_speed_coeff_pa_s_per_m" => {
                self.friction.fmep_piston_speed_coeff_pa_s_per_m
            }
            "friction.fmep_piston_speed_sq_coeff" => self.friction.fmep_piston_speed_sq_coeff,

            "air_path.intake_manifold_pressure_pa" => self.air_path.intake_manifold_pressure_pa,
            "air_path.intake_manifold_temperature_k" => self.air_path.intake_manifold_temperature_k,
            "air_path.exhaust_manifold_pressure_pa" => self.air_path.exhaust_manifold_pressure_pa,
            "air_path.crankcase_pressure_pa" => self.air_path.crankcase_pressure_pa,
            "air_path.gas_constant_j_per_kg_k" => self.air_path.gas_constant_j_per_kg_k,
            "air_path.volumetric_efficiency" => self.air_path.volumetric_efficiency,

            "governor.idle_target_rpm" => self.governor.idle_target_rpm,
            "governor.idle_p_gain_mg_per_rad_s" => self.governor.idle_p_gain_mg_per_rad_s,
            "governor.idle_i_gain_mg_per_rad" => self.governor.idle_i_gain_mg_per_rad,
            "governor.idle_integral_limit_mg" => self.governor.idle_integral_limit_mg,
            "governor.overspeed_taper_start_rpm" => self.governor.overspeed_taper_start_rpm,
            "governor.overspeed_cutoff_rpm" => self.governor.overspeed_cutoff_rpm,

            "load.accessory_torque_constant_nm" => self.load.accessory_torque_constant_nm,
            "load.accessory_torque_per_rad_s" => self.load.accessory_torque_per_rad_s,
            "load.max_external_load_nm" => self.load.max_external_load_nm,
            "load.starter_torque_nm" => self.load.starter_torque_nm,
            "load.starter_cutout_rpm" => self.load.starter_cutout_rpm,

            "inertia.rotating_inertia_kg_m2" => self.inertia.rotating_inertia_kg_m2,
            "inertia.complete_engine_mass_kg" => self.inertia.complete_engine_mass_kg,

            "limits.max_combustion_pressure_pa" => self.limits.max_combustion_pressure_pa,
            "limits.max_rpm" => self.limits.max_rpm,
            "limits.max_gas_temperature_k" => self.limits.max_gas_temperature_k,

            "solver.fixed_step_s" => self.solver.fixed_step_s,
            "solver.max_steps_per_batch" => f64::from(self.solver.max_steps_per_batch),

            "audio.reference_spl_db" => self.audio.reference_spl_db,
            "audio.exhaust_gain" => self.audio.exhaust_gain,

            "rated.max_power_w" => self.rated.max_power_w,
            "rated.max_power_hp" => self.rated.max_power_hp,
            "rated.max_torque_nm" => self.rated.max_torque_nm,

            _ => return None,
        };
        Some(v)
    }
}
