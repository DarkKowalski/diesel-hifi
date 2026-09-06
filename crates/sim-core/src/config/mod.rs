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
//! - **v4** (Milestone 4) — the staged decompression engine brake and a rigid
//!   truck driveline, as two new sections. Nothing is removed this time, but a v3
//!   document has neither section and `serde` rejects it, so this is still a
//!   breaking change to the document.
//!
//!   The v3 header predicted Milestone 4 would only grow existing sections and
//!   need no bump. That was wrong in one respect: the brake and the driveline are
//!   subsystems in their own right, and folding a brake cam contour into
//!   `valvetrain` or a vehicle mass into `load` would have made both sections
//!   describe two unrelated things.
//!
//! The *shape* of the document has not changed across any of these: same
//! versioned sections, same provenance rules, same [`Schedule`] type, and an
//! additive snapshot. What changes is which calibration fields exist, because a
//! field that no longer describes anything should not sit in a provenance table
//! that claims to describe the model.

pub mod paths;
pub mod provenance;
pub mod schedule;
pub mod validate;

use serde::{Deserialize, Serialize};

pub use provenance::{ProvenanceEntry, ProvenanceReport, ProvenanceStatus, SourceRecord};
pub use schedule::Schedule;
pub use validate::ValidatedConfig;

/// The only configuration schema version understood by this build.
pub const SCHEMA_VERSION: u32 = 4;

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

/// Staged decompression engine brake.
///
/// The manual describes the mechanism in unusual detail: brake cams with **two
/// peaks**, the first opening an exhaust valve around the start of the
/// compression stroke so that "exhaust flows out of the exhaust manifold back
/// into the cylinder", the second opening it "shortly before ending the
/// compression stroke" so that "part of the compression pressure is reduced".
/// Charge, then dump — the piston pays for a compression it never gets back.
///
/// What the manual publishes: the 1000 rpm activation floor, that stage I acts on
/// cylinders 1 to 3, and the two brake-power anchors for the fitted variant. It
/// publishes no cam contour, no lift, no timing and no MCM target, so everything
/// describing the *shape* of the event is calibrated.
///
/// The anchors are recorded here as published reference values and are read only
/// by tests. No solver code may consult them: braking torque has to emerge from
/// cylinder pressure through slider-crank geometry, exactly like firing torque,
/// and a model that looked up its own answer would prove nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineBrake {
    /// Fitted variant code: `M5U` standard, or `M5V` high performance.
    ///
    /// The manual is explicit that "the hardware of the two systems is identical"
    /// and that they differ only by "a different code-controlled data record", so
    /// this selects data, never behaviour.
    pub variant: String,
    /// Published: the brake operates above 1000 rpm.
    pub min_speed_rpm: f64,
    /// Published: stage I brakes on cylinders 1 to 3.
    pub stage1_cylinder_count: u32,

    /// Published lower anchor: engine speed and absorbed power.
    pub anchor_low_rpm: f64,
    pub anchor_low_power_w: f64,
    /// Published upper anchor: engine speed and absorbed power.
    pub anchor_high_rpm: f64,
    pub anchor_high_power_w: f64,

    /// Signed cycle angle of the charging lobe — the manual's first peak.
    /// Negative is before firing TDC, so this sits early in the compression
    /// stroke, after intake valve closing.
    pub charge_center_rad: f64,
    /// Signed cycle angle of the release lobe — the manual's second peak, just
    /// before firing TDC.
    pub release_center_rad: f64,
    /// Half-width of the charging lobe in crank angle.
    ///
    /// Separate from the release lobe's width because the two peaks do different
    /// jobs and the manual gives no reason they would be alike. The charging lobe
    /// has to pass enough gas to fill the cylinder, and the time it is given
    /// shrinks with engine speed, so it wants to be wide. The release lobe has to
    /// dump a cylinder that is already at peak compression, which takes very
    /// little open area and wants to happen late; widening it instead starts the
    /// dump early and throws away compression work that has not been done yet.
    pub charge_width_rad: f64,
    /// Half-width of the release lobe in crank angle.
    pub release_width_rad: f64,
    /// Effective flow area of the braked valve at full lift.
    ///
    /// One valve, not the pair: the manual says "one of the two exhaust valves is
    /// opened", and that is also why this is far smaller than
    /// `valvetrain.exhaust_effective_area_m2`.
    pub effective_area_m2: f64,

    /// Wastegate boost setpoint commanded in each stage, absolute pascals.
    ///
    /// The manual has the MCM actuating the wastegate in stage III to raise
    /// cylinder pressure and names "the boost pressure and the turbocharger speed"
    /// as its controlled variables, and says the high-performance variant raises
    /// cylinder pressure "in all engine brake stages". One setpoint per stage is
    /// therefore literally the data record the manual describes: M5U leaves stages
    /// I and II at ambient and lifts stage III; M5V lifts all three.
    pub stage1_boost_target_pa: f64,
    pub stage2_boost_target_pa: f64,
    pub stage3_boost_target_pa: f64,
    /// Factor applied to the wastegate controller's gains and slew rate while the
    /// brake is engaged.
    ///
    /// The brake carries its own positive feedback: more boost packs the cylinder
    /// harder, which dumps more energy into the turbine, which makes more boost.
    /// The wastegate gains calibrated for the fuelled engine are far too hot for
    /// that plant, and at full gain the loop hunted above 1800 rpm — badly enough
    /// that absorbed power at the published 2300 rpm anchor swung between 282 and
    /// 300 kW depending only on where in the oscillation the measurement landed.
    /// Longer settling did not help, because it was a limit cycle and not a
    /// transient.
    ///
    /// Detuning the loop is the textbook answer to a plant whose gain has changed
    /// by an order of magnitude, and it is preferable to commanding a fixed valve
    /// position: with the wastegate held open-loop, absorbed power at 2300 rpm
    /// swings roughly threefold between positions 0.35 and 0.25 as the turbine
    /// reaches its speed clamp, which is not a control axis a calibration can sit
    /// on.
    pub wastegate_gain_scale: f64,

    /// EGR valve position commanded in each stage.
    ///
    /// The manual actuates the EGR positioner alongside the wastegate "whereby
    /// the fill level of the cylinder is increased again".
    pub stage1_egr_command: f64,
    pub stage2_egr_command: f64,
    pub stage3_egr_command: f64,
}

/// Truck driveline and road load.
///
/// Entirely calibrated. The source is an engine document: it publishes nothing
/// about the vehicle the engine is fitted to. These values describe a plausible
/// fully-laden European tractor-trailer, and are the reason the engine brake has
/// something to brake against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Driveline {
    /// Gross combination mass, kg.
    pub vehicle_mass_kg: f64,
    pub wheel_radius_m: f64,
    pub rolling_resistance_coeff: f64,
    /// Drag area, the product of drag coefficient and frontal area, m^2.
    pub drag_area_m2: f64,
    pub final_drive_ratio: f64,
    /// Gearbox ratios, lowest gear first. Gear 0 is neutral and is not listed.
    pub gear_ratios: Vec<f64>,
    pub driveline_efficiency: f64,
    /// Steepest road grade the controls will accept, percent, in both directions.
    pub max_grade_percent: f64,
}

impl Driveline {
    /// Total reduction from crank to wheel for a gear, or `None` in neutral.
    ///
    /// Gears are numbered from 1; gear 0 means neutral, and so does any gear
    /// beyond the fitted ratios.
    pub fn total_ratio(&self, gear: u32) -> Option<f64> {
        if gear == 0 {
            return None;
        }
        let ratio = *self.gear_ratios.get((gear - 1) as usize)?;
        let total = ratio * self.final_drive_ratio;
        if total > 0.0 {
            Some(total)
        } else {
            None
        }
    }

    /// Number of forward gears fitted.
    pub fn gear_count(&self) -> u32 {
        self.gear_ratios.len() as u32
    }
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

/// One structural mode of the engine's outer surface.
///
/// A near-step pressure rise inside a stiff iron box excites bending and
/// breathing modes of the block, head, and covers. They ring for a few
/// milliseconds and radiate straight off the engine's skin, by a path that never
/// goes near the exhaust. That is combustion noise, and it is what makes the ear
/// say *diesel* rather than *engine*.
///
/// Each mode is one two-pole resonator. Nothing here is published: the manual
/// says nothing about how the engine sounds, let alone about its mode shapes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StructuralMode {
    /// Ring frequency.
    pub frequency_hz: f64,
    /// Sharpness. Higher rings longer; an iron structure is in the tens.
    pub q: f64,
    /// Weight of this mode in the summed structural signal.
    pub gain: f64,
}

/// Engine acoustic model.
///
/// Two radiating paths. The exhaust source is the computed blowdown through the
/// exhaust ports; the structural source is the computed cylinder pressure
/// shaking the engine's outer surface. These values shape both into one signal.
/// All calibrated: the manual publishes nothing about how the engine sounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioCalibration {
    pub reference_spl_db: f64,
    pub exhaust_gain: f64,
    /// Removes the standing pressure offset, leaving the pulses.
    pub highpass_cutoff_hz: f64,
    /// Muffler roll-off.
    pub lowpass_cutoff_hz: f64,
    /// Soft-clip knee keeping samples inside [-1, 1] without hard clipping.
    pub soft_clip_knee: f64,
    /// Level of the structural path against the exhaust path.
    pub structural_gain: f64,
    /// Modal bank the cylinder-pressure rise rings.
    pub structural_modes: Vec<StructuralMode>,
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
    pub engine_brake: EngineBrake,
    pub driveline: Driveline,
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
    /// Returns `None` for known non-scalar paths (firing order, gear ratios,
    /// schedules, the structural mode bank, the brake variant code, and the
    /// heat-transfer enable flag) and for unknown paths. Provenance
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

            "engine_brake.variant" => return None,
            "engine_brake.min_speed_rpm" => self.engine_brake.min_speed_rpm,
            "engine_brake.stage1_cylinder_count" => {
                f64::from(self.engine_brake.stage1_cylinder_count)
            }
            "engine_brake.anchor_low_rpm" => self.engine_brake.anchor_low_rpm,
            "engine_brake.anchor_low_power_w" => self.engine_brake.anchor_low_power_w,
            "engine_brake.anchor_high_rpm" => self.engine_brake.anchor_high_rpm,
            "engine_brake.anchor_high_power_w" => self.engine_brake.anchor_high_power_w,
            "engine_brake.charge_center_rad" => self.engine_brake.charge_center_rad,
            "engine_brake.release_center_rad" => self.engine_brake.release_center_rad,
            "engine_brake.charge_width_rad" => self.engine_brake.charge_width_rad,
            "engine_brake.release_width_rad" => self.engine_brake.release_width_rad,
            "engine_brake.effective_area_m2" => self.engine_brake.effective_area_m2,
            "engine_brake.stage1_boost_target_pa" => self.engine_brake.stage1_boost_target_pa,
            "engine_brake.stage2_boost_target_pa" => self.engine_brake.stage2_boost_target_pa,
            "engine_brake.stage3_boost_target_pa" => self.engine_brake.stage3_boost_target_pa,
            "engine_brake.wastegate_gain_scale" => self.engine_brake.wastegate_gain_scale,
            "engine_brake.stage1_egr_command" => self.engine_brake.stage1_egr_command,
            "engine_brake.stage2_egr_command" => self.engine_brake.stage2_egr_command,
            "engine_brake.stage3_egr_command" => self.engine_brake.stage3_egr_command,

            "driveline.vehicle_mass_kg" => self.driveline.vehicle_mass_kg,
            "driveline.wheel_radius_m" => self.driveline.wheel_radius_m,
            "driveline.rolling_resistance_coeff" => self.driveline.rolling_resistance_coeff,
            "driveline.drag_area_m2" => self.driveline.drag_area_m2,
            "driveline.final_drive_ratio" => self.driveline.final_drive_ratio,
            "driveline.gear_ratios" => return None,
            "driveline.driveline_efficiency" => self.driveline.driveline_efficiency,
            "driveline.max_grade_percent" => self.driveline.max_grade_percent,

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
            "audio.structural_gain" => self.audio.structural_gain,
            "audio.structural_modes" => return None,

            "rated.max_power_w" => self.rated.max_power_w,
            "rated.max_power_hp" => self.rated.max_power_hp,
            "rated.max_torque_nm" => self.rated.max_torque_nm,

            _ => return None,
        };
        Some(v)
    }
}
