//! The compact telemetry snapshot returned across the WASM boundary.
//!
//! The snapshot is versioned and additive: Milestone 3 boost/EGR fields and
//! Milestone 4 engine-brake fields slot in as new members without a protocol
//! change.
//!
//! Snapshots are built once per batch, never per step.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, SimError};

/// Snapshot schema version. Bump only on a breaking change.
pub const SNAPSHOT_VERSION: u32 = 2;

/// Coarse run state, derived from crank speed and controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Stopped,
    Cranking,
    Running,
    Fault,
}

/// A latched fault carried in the snapshot so the UI can show it without
/// needing the error to be thrown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimFault {
    pub code: ErrorCode,
    pub message: String,
}

impl From<&SimError> for SimFault {
    fn from(e: &SimError) -> Self {
        Self {
            code: e.code,
            message: e.message.clone(),
        }
    }
}

/// Compact telemetry for one batch boundary.
///
/// Instantaneous torque swings by more than a thousand newton-metres inside a
/// cycle, so the `*_cycle_*` members are the ones worth reading for anything
/// quantitative. They are whole-cycle averages and are only meaningful once
/// `cycle_valid` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    pub config_id: String,
    pub state: RunState,

    pub sim_time_s: f64,
    /// Crank angle wrapped into `[0, 4*PI)`, one full four-stroke cycle.
    pub crank_angle_rad: f64,
    pub rpm: f64,
    pub steps_advanced: u64,

    /// Instantaneous pressure per cylinder, index 0 = cylinder 1.
    pub cylinder_pressure_pa: Vec<f64>,
    /// Motored (no-combustion) pressure per cylinder, for comparison.
    pub cylinder_motored_pressure_pa: Vec<f64>,
    /// Peak pressure observed during the cycle in progress.
    pub peak_pressure_pa_cycle: f64,
    /// Peak pressure observed since the last reset.
    pub peak_pressure_pa_session: f64,
    /// Peak single-zone gas temperature during the cycle in progress.
    pub peak_gas_temperature_k: f64,

    // Instantaneous torque terms.
    pub torque_gas_nm: f64,
    pub torque_pumping_nm: f64,
    pub torque_friction_nm: f64,
    pub torque_accessory_nm: f64,
    pub torque_starter_nm: f64,
    pub torque_load_nm: f64,
    pub torque_net_nm: f64,

    // Whole-cycle averages from the most recently completed cycle.
    pub cycle_valid: bool,
    pub cycles_completed: u64,
    pub indicated_torque_cycle_nm: f64,
    pub brake_torque_cycle_nm: f64,
    pub brake_power_cycle_w: f64,
    pub imep_pa: f64,
    pub bmep_pa: f64,
    pub bsfc_g_per_kwh: f64,

    // Fuelling and injection.
    pub fuel_per_cycle_mg: f64,
    pub fuel_demand_mg: f64,
    /// APCRS variant selected for the most recent injection: the published
    /// "with or without additional pressure amplification" distinction.
    pub injection_variant: String,
    pub injection_pressure_pa: f64,
    pub ignition_delay_rad: f64,
    pub premixed_fraction: f64,

    // Air path.
    pub intake_pressure_pa: f64,
    pub intake_temperature_k: f64,
    pub exhaust_pressure_pa: f64,
    pub exhaust_temperature_k: f64,
    pub residual_fraction: f64,

    // Turbocharger and EGR. All of this is now computed rather than prescribed.
    /// Intake pressure above ambient.
    pub boost_pressure_pa: f64,
    pub turbo_shaft_rad_per_s: f64,
    /// Wastegate opening, `0..=1`.
    pub wastegate_position: f64,
    /// Recirculated mass over total charge mass.
    pub egr_rate: f64,
    /// EGR valve opening, `0..=1`.
    pub egr_valve_position: f64,
    /// Burned-gas mass fraction in the intake manifold; what dilutes the oxygen
    /// the smoke limit sees.
    pub intake_burned_fraction: f64,
    pub compressor_flow_kg_per_s: f64,
    pub turbine_flow_kg_per_s: f64,
    pub egr_flow_kg_per_s: f64,

    // Engine brake and driveline.
    /// Brake stage actually in force, `0` when the brake is not operating.
    ///
    /// This is what the brake *did*, not what the driver asked for: the switch
    /// can be at stage III while the published operating conditions hold the
    /// brake off, and the two must be distinguishable in telemetry.
    pub brake_stage_active: u8,
    /// Whether the decompression brake is operating at all.
    pub brake_active: bool,
    /// Cycle-averaged power the engine is absorbing, watts, positive while
    /// braking and zero otherwise.
    pub brake_absorbed_power_w: f64,
    /// Road speed, m/s. Zero in neutral.
    pub vehicle_speed_m_per_s: f64,
    /// Road load reflected to the crank. Negative on a descent.
    pub torque_driveline_nm: f64,
    /// Vehicle inertia reflected to the crankshaft, kg m^2.
    pub reflected_inertia_kg_m2: f64,
    /// Whether a gear is engaged.
    pub gear_engaged: bool,

    /// Exhaust level relative to the configured reference. Audio samples
    /// themselves are drained separately; the snapshot stays compact.
    pub audio_level_db: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<SimFault>,
}
