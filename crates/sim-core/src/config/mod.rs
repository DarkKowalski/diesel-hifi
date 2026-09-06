//! Versioned, validated engine configuration.
//!
//! All simulation inputs live here. The solver reads only [`ValidatedConfig`];
//! it never embeds engine-specific calibration and never branches on an engine
//! ID (SPEC section 5).
//!
//! # Schema history
//!
//! - **v1** (Milestone 1) — placeholder combustion: a single cosine burn with
//!   lumped polytropic exponents.
//! - **v2** (Milestone 2) — injection, ignition delay, double-Wiebe heat release,
//!   Woschni wall heat transfer, temperature-dependent specific heats, and a
//!   prescribed boost schedule. The placeholder combustion fields were removed
//!   rather than extended, which is why the version was bumped rather than the
//!   sections grown additively.
//! - **v3** (Milestone 3) — wastegate turbocharger and cooled EGR dynamics, plus
//!   the exhaust acoustic source. Boost stops being an input: the prescribed
//!   response, charge-temperature fit, and back-pressure fit are removed because
//!   a compressor, an intercooler, and a turbine now compute them.
//!   `boost_target_schedule` survives with a more honest meaning — it is the
//!   ECU's wastegate setpoint, which is what the manual describes the MCM
//!   regulating to.
//!
//! The *shape* of the document has not changed across any of these: same
//! versioned sections, same provenance rules, same [`Schedule`] type, and an
//! additive snapshot. What changes is which calibration fields exist, because a
//! field that no longer describes anything should not sit in a provenance table
//! that claims to describe the model.
//!
//! Milestone 4 (engine brake, driveline) is expected to add fields to existing
//! sections and should not need another version bump.

pub mod paths;
pub mod provenance;
pub mod schedule;
pub mod validate;

use serde::{Deserialize, Serialize};

pub use provenance::{ProvenanceEntry, ProvenanceReport, ProvenanceStatus, SourceRecord};
pub use schedule::Schedule;
pub use validate::ValidatedConfig;

/// The only configuration schema version understood by this build.
pub const SCHEMA_VERSION: u32 = 3;

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

/// Valve counts (published) and the simplified gas-exchange boundaries.
/// Real valve events are not published in the source manual.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Valvetrain {
    pub intake_valves_per_cylinder: u32,
    pub exhaust_valves_per_cylinder: u32,
    /// Signed crank angle from firing TDC at which the cylinder becomes closed.
    /// Negative is before firing TDC (during the compression stroke).
    pub intake_valve_close_rad: f64,
    /// Signed crank angle from firing TDC at which the cylinder reopens.
    pub exhaust_valve_open_rad: f64,
    /// Crank angle over which the exhaust valve ramps between shut and full
    /// lift.
    ///
    /// Short compared with the exhaust window: the valve cracks open quickly,
    /// which is what makes blowdown a sharp pulse rather than a slow hump.
    pub exhaust_ramp_rad: f64,
    /// Effective flow area of one cylinder's exhaust port at full lift.
    ///
    /// Milestone 2 needed only the *angle* at which the cylinder reopens. The
    /// acoustic source needs an area as well, because the exhaust pulse is a
    /// flow through a port, not an event on a crank wheel.
    pub exhaust_effective_area_m2: f64,
}

/// Injection system.
///
/// The manual publishes the *structure* — two APCRS variants, "with or without
/// additional pressure amplification", and MCM-scheduled quantity and timing —
/// plus the 900 bar rail and 2100 bar amplified pressure limits. Every other
/// number here is calibrated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Injection {
    pub rail_pressure_max_pa: f64,
    pub amplified_pressure_max_pa: f64,
    pub fuel_lower_heating_value_j_per_kg: f64,
    pub fuel_density_kg_m3: f64,
    pub max_fuel_mg_per_cycle: f64,
    /// Minimum air/fuel ratio permitted before fuelling is clipped (smoke limit).
    pub smoke_limit_afr: f64,
    /// Air/fuel ratio at which the fuel exactly consumes the air.
    ///
    /// Needed once exhaust is recirculated: it converts fuel burned into mass of
    /// combustion products, which is what actually displaces oxygen in the
    /// intake. A diesel runs lean, so its exhaust still carries usable oxygen,
    /// and treating recirculated gas as inert would overstate the EGR penalty.
    pub stoichiometric_afr: f64,
    pub nozzle_hole_count: u32,
    pub nozzle_hole_diameter_m: f64,
    pub discharge_coefficient: f64,
    /// Fuel quantity at or above which the amplified-pressure variant is chosen.
    pub amplified_variant_min_fuel_mg: f64,
    /// Engine speed (rpm) to base start of injection (rad, negative before TDC).
    pub soi_schedule: Schedule,
    /// Retard applied to start of injection per milligram of fuel. Positive
    /// values move injection later, which is what keeps peak cylinder pressure
    /// inside the published envelope at high load.
    pub soi_load_retard_rad_per_mg: f64,
}

/// Double-Wiebe heat release.
///
/// The premixed fraction is not assumed: it is the fuel physically injected
/// during the ignition delay, capped by `max_premixed_fraction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Combustion {
    pub combustion_efficiency: f64,
    /// Fuel cetane number, feeding the Hardenberg-Hase activation energy.
    pub cetane_number: f64,
    pub premixed_wiebe_shape: f64,
    pub premixed_wiebe_efficiency: f64,
    pub premixed_duration_rad: f64,
    pub diffusion_wiebe_shape: f64,
    pub diffusion_wiebe_efficiency: f64,
    /// Diffusion burn lengthens with the injected quantity.
    pub diffusion_duration_rad_per_mg: f64,
    pub diffusion_duration_min_rad: f64,
    pub max_premixed_fraction: f64,
}

/// Woschni convective wall heat transfer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeatTransfer {
    /// Disabling this is a test affordance, not an operating mode.
    pub enabled: bool,
    pub woschni_c1_closed: f64,
    pub woschni_c1_gas_exchange: f64,
    pub woschni_c2: f64,
    pub wall_temperature_k: f64,
}

/// Working-gas properties with a linear specific-heat model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GasProperties {
    pub gas_constant_j_per_kg_k: f64,
    pub cv_reference_j_per_kg_k: f64,
    pub cv_slope_j_per_kg_k2: f64,
    pub reference_temperature_k: f64,
}

/// Air path.
///
/// Milestone 3 makes the manifolds real control volumes. Pressures are states
/// integrated from mass flow rather than prescribed, which is also what removes
/// the fuelling/air algebraic loop Milestone 2 broke with a first-order lag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirPath {
    pub ambient_pressure_pa: f64,
    pub ambient_temperature_k: f64,
    /// Engine speed (rpm) to the ECU's full-load boost setpoint (Pa absolute).
    ///
    /// In Milestone 2 this was written straight into manifold pressure. It is
    /// now what the wastegate controller regulates *towards*, which is what the
    /// manual describes the MCM doing via the boost pressure positioner.
    pub boost_target_schedule: Schedule,
    /// Intake manifold and charge-air housing volume, as one control volume.
    pub intake_manifold_volume_m3: f64,
    /// Exhaust manifold volume upstream of the turbine, as one control volume.
    pub exhaust_manifold_volume_m3: f64,
    /// Charge-air cooler effectiveness.
    pub intercooler_effectiveness: f64,
    /// Coolant temperature, the heat sink for both coolers.
    pub coolant_temperature_k: f64,
    /// Lumped restriction downstream of the turbine: `dp = k * m_dot^2`.
    ///
    /// This stands for the muffler and the aftertreatment can as a flow
    /// resistance only. No DPF loading, SCR, dosing, or regeneration is
    /// modelled; SPEC section 3 excludes complete aftertreatment from the MVP.
    pub exhaust_restriction_pa_per_kg2_s2: f64,
    /// Pressure on the underside of the piston, used for the net gas force.
    pub crankcase_pressure_pa: f64,
    pub volumetric_efficiency: f64,
}

/// Wastegate turbocharger.
///
/// The manual publishes the architecture — a single turbine and compressor on a
/// joint shaft, a charge-air cooler, and boost regulated by a wastegate the MCM
/// drives through a vacuum cell and linkage. It publishes no geometry, no
/// efficiency, no inertia, and no boost pressure, so every number here is
/// calibrated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Turbo {
    pub shaft_inertia_kg_m2: f64,
    pub compressor_wheel_diameter_m: f64,
    pub compressor_efficiency: f64,
    /// Head coefficient relating tip speed to pressure rise.
    pub compressor_head_coefficient: f64,
    /// Choke flow scaling: maximum corrected flow per rad/s of shaft speed.
    pub compressor_max_flow_kg_s_per_rad_s: f64,
    /// Turbine effective flow area — its swallowing capacity.
    pub turbine_effective_area_m2: f64,
    pub turbine_efficiency: f64,
    pub bearing_friction_nm_per_rad_s: f64,
    /// Overspeed guard for the shaft, mirroring the published turbocharger
    /// protection function.
    pub max_shaft_speed_rad_per_s: f64,
    pub wastegate_max_area_m2: f64,
    pub wastegate_p_gain_per_pa: f64,
    pub wastegate_i_gain_per_pa_s: f64,
    /// Actuator rate limit. The vacuum cell and linkage cannot move instantly.
    pub wastegate_slew_per_s: f64,
}

/// Cooled high-pressure exhaust gas recirculation.
///
/// The cooler inlet and outlet temperatures are the only published values this
/// subsystem contributes: the manual states cooling "from a temperature of about
/// 650 C down to about 170 C". The recirculation *rate* is not published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Egr {
    /// Published: about 650 C at the cooler inlet.
    pub cooler_inlet_temperature_k: f64,
    /// Published: about 170 C at the cooler outlet.
    pub cooler_outlet_temperature_k: f64,
    /// Derived from the published inlet/outlet pair against coolant temperature.
    pub cooler_effectiveness: f64,
    /// Engine speed (rpm) to target recirculation rate, mass fraction.
    pub rate_schedule: Schedule,
    pub valve_max_area_m2: f64,
    pub valve_discharge_coefficient: f64,
    pub rate_p_gain: f64,
    pub rate_i_gain_per_s: f64,
    /// Hard ceiling on recirculation rate. Too much exhaust spoils combustion
    /// and raises soot, CO and HC, which the manual states explicitly.
    pub max_rate: f64,
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

/// Chen-Flynn style friction mean effective pressure terms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Friction {
    pub fmep_constant_pa: f64,
    pub fmep_peak_pressure_coeff: f64,
    pub fmep_piston_speed_coeff_pa_s_per_m: f64,
    pub fmep_piston_speed_sq_coeff: f64,
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

/// Exhaust acoustic model.
///
/// The source is the computed blowdown through the exhaust ports; these values
/// shape it into a signal. All calibrated: the manual publishes nothing about
/// how the engine sounds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioCalibration {
    pub reference_spl_db: f64,
    pub exhaust_gain: f64,
    /// Removes the standing pressure offset, leaving the pulses.
    pub highpass_cutoff_hz: f64,
    /// Muffler roll-off.
    pub lowpass_cutoff_hz: f64,
    /// Soft-clip knee keeping samples inside [-1, 1] without hard clipping.
    pub soft_clip_knee: f64,
}

/// Published rated output.
///
/// The manual does not publish the engine speeds at which these occur. The
/// speeds this model reaches them at are a calibration outcome, recorded as
/// such, and are never presented as OEM data.
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
    pub heat_transfer: HeatTransfer,
    pub gas: GasProperties,
    pub friction: Friction,
    pub air_path: AirPath,
    pub turbo: Turbo,
    pub egr: Egr,
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
    /// Returns `None` for known non-scalar paths (firing order, schedules, and
    /// the heat-transfer enable flag) and for unknown paths. Provenance
    /// range-containment is skipped for those.
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
            "valvetrain.exhaust_ramp_rad" => self.valvetrain.exhaust_ramp_rad,
            "valvetrain.exhaust_effective_area_m2" => self.valvetrain.exhaust_effective_area_m2,

            "injection.rail_pressure_max_pa" => self.injection.rail_pressure_max_pa,
            "injection.amplified_pressure_max_pa" => self.injection.amplified_pressure_max_pa,
            "injection.fuel_lower_heating_value_j_per_kg" => {
                self.injection.fuel_lower_heating_value_j_per_kg
            }
            "injection.fuel_density_kg_m3" => self.injection.fuel_density_kg_m3,
            "injection.max_fuel_mg_per_cycle" => self.injection.max_fuel_mg_per_cycle,
            "injection.smoke_limit_afr" => self.injection.smoke_limit_afr,
            "injection.stoichiometric_afr" => self.injection.stoichiometric_afr,
            "injection.nozzle_hole_count" => f64::from(self.injection.nozzle_hole_count),
            "injection.nozzle_hole_diameter_m" => self.injection.nozzle_hole_diameter_m,
            "injection.discharge_coefficient" => self.injection.discharge_coefficient,
            "injection.amplified_variant_min_fuel_mg" => {
                self.injection.amplified_variant_min_fuel_mg
            }
            "injection.soi_schedule" => return None,
            "injection.soi_load_retard_rad_per_mg" => self.injection.soi_load_retard_rad_per_mg,

            "combustion.combustion_efficiency" => self.combustion.combustion_efficiency,
            "combustion.cetane_number" => self.combustion.cetane_number,
            "combustion.premixed_wiebe_shape" => self.combustion.premixed_wiebe_shape,
            "combustion.premixed_wiebe_efficiency" => self.combustion.premixed_wiebe_efficiency,
            "combustion.premixed_duration_rad" => self.combustion.premixed_duration_rad,
            "combustion.diffusion_wiebe_shape" => self.combustion.diffusion_wiebe_shape,
            "combustion.diffusion_wiebe_efficiency" => self.combustion.diffusion_wiebe_efficiency,
            "combustion.diffusion_duration_rad_per_mg" => {
                self.combustion.diffusion_duration_rad_per_mg
            }
            "combustion.diffusion_duration_min_rad" => self.combustion.diffusion_duration_min_rad,
            "combustion.max_premixed_fraction" => self.combustion.max_premixed_fraction,

            "heat_transfer.enabled" => return None,
            "heat_transfer.woschni_c1_closed" => self.heat_transfer.woschni_c1_closed,
            "heat_transfer.woschni_c1_gas_exchange" => self.heat_transfer.woschni_c1_gas_exchange,
            "heat_transfer.woschni_c2" => self.heat_transfer.woschni_c2,
            "heat_transfer.wall_temperature_k" => self.heat_transfer.wall_temperature_k,

            "gas.gas_constant_j_per_kg_k" => self.gas.gas_constant_j_per_kg_k,
            "gas.cv_reference_j_per_kg_k" => self.gas.cv_reference_j_per_kg_k,
            "gas.cv_slope_j_per_kg_k2" => self.gas.cv_slope_j_per_kg_k2,
            "gas.reference_temperature_k" => self.gas.reference_temperature_k,

            "friction.fmep_constant_pa" => self.friction.fmep_constant_pa,
            "friction.fmep_peak_pressure_coeff" => self.friction.fmep_peak_pressure_coeff,
            "friction.fmep_piston_speed_coeff_pa_s_per_m" => {
                self.friction.fmep_piston_speed_coeff_pa_s_per_m
            }
            "friction.fmep_piston_speed_sq_coeff" => self.friction.fmep_piston_speed_sq_coeff,

            "air_path.ambient_pressure_pa" => self.air_path.ambient_pressure_pa,
            "air_path.ambient_temperature_k" => self.air_path.ambient_temperature_k,
            "air_path.boost_target_schedule" => return None,
            "air_path.intake_manifold_volume_m3" => self.air_path.intake_manifold_volume_m3,
            "air_path.exhaust_manifold_volume_m3" => self.air_path.exhaust_manifold_volume_m3,
            "air_path.intercooler_effectiveness" => self.air_path.intercooler_effectiveness,
            "air_path.coolant_temperature_k" => self.air_path.coolant_temperature_k,
            "air_path.exhaust_restriction_pa_per_kg2_s2" => {
                self.air_path.exhaust_restriction_pa_per_kg2_s2
            }
            "air_path.crankcase_pressure_pa" => self.air_path.crankcase_pressure_pa,
            "air_path.volumetric_efficiency" => self.air_path.volumetric_efficiency,

            "turbo.shaft_inertia_kg_m2" => self.turbo.shaft_inertia_kg_m2,
            "turbo.compressor_wheel_diameter_m" => self.turbo.compressor_wheel_diameter_m,
            "turbo.compressor_efficiency" => self.turbo.compressor_efficiency,
            "turbo.compressor_head_coefficient" => self.turbo.compressor_head_coefficient,
            "turbo.compressor_max_flow_kg_s_per_rad_s" => {
                self.turbo.compressor_max_flow_kg_s_per_rad_s
            }
            "turbo.turbine_effective_area_m2" => self.turbo.turbine_effective_area_m2,
            "turbo.turbine_efficiency" => self.turbo.turbine_efficiency,
            "turbo.bearing_friction_nm_per_rad_s" => self.turbo.bearing_friction_nm_per_rad_s,
            "turbo.max_shaft_speed_rad_per_s" => self.turbo.max_shaft_speed_rad_per_s,
            "turbo.wastegate_max_area_m2" => self.turbo.wastegate_max_area_m2,
            "turbo.wastegate_p_gain_per_pa" => self.turbo.wastegate_p_gain_per_pa,
            "turbo.wastegate_i_gain_per_pa_s" => self.turbo.wastegate_i_gain_per_pa_s,
            "turbo.wastegate_slew_per_s" => self.turbo.wastegate_slew_per_s,

            "egr.cooler_inlet_temperature_k" => self.egr.cooler_inlet_temperature_k,
            "egr.cooler_outlet_temperature_k" => self.egr.cooler_outlet_temperature_k,
            "egr.cooler_effectiveness" => self.egr.cooler_effectiveness,
            "egr.rate_schedule" => return None,
            "egr.valve_max_area_m2" => self.egr.valve_max_area_m2,
            "egr.valve_discharge_coefficient" => self.egr.valve_discharge_coefficient,
            "egr.rate_p_gain" => self.egr.rate_p_gain,
            "egr.rate_i_gain_per_s" => self.egr.rate_i_gain_per_s,
            "egr.max_rate" => self.egr.max_rate,

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
            "audio.highpass_cutoff_hz" => self.audio.highpass_cutoff_hz,
            "audio.lowpass_cutoff_hz" => self.audio.lowpass_cutoff_hz,
            "audio.soft_clip_knee" => self.audio.soft_clip_knee,

            "rated.max_power_w" => self.rated.max_power_w,
            "rated.max_power_hp" => self.rated.max_power_hp,
            "rated.max_torque_nm" => self.rated.max_torque_nm,

            _ => return None,
        };
        Some(v)
    }
}
