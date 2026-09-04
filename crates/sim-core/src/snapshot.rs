//! The compact telemetry snapshot returned across the WASM boundary.
//!
//! The snapshot is versioned and additive: Milestone 3 boost/EGR fields and
//! Milestone 4 engine-brake fields slot in as new optional members without a
//! protocol change.
//!
//! Snapshots are built once per batch, never per step.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, SimError};

/// Snapshot schema version. Bump only on a breaking change.
pub const SNAPSHOT_VERSION: u32 = 1;

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
    /// Peak pressure observed during the cycle in progress.
    pub peak_pressure_pa_cycle: f64,
    /// Peak pressure observed since the last reset.
    pub peak_pressure_pa_session: f64,

    /// Torque from cylinder pressure during the closed period.
    pub torque_gas_nm: f64,
    /// Torque from cylinder pressure during gas exchange (pumping), reported
    /// separately but taken from the same pressure integral, not double counted.
    pub torque_pumping_nm: f64,
    pub torque_friction_nm: f64,
    pub torque_accessory_nm: f64,
    pub torque_starter_nm: f64,
    pub torque_load_nm: f64,
    pub torque_net_nm: f64,

    pub fuel_per_cycle_mg: f64,
    pub fuel_demand_mg: f64,
    pub intake_pressure_pa: f64,
    pub exhaust_pressure_pa: f64,
    /// Peak single-zone gas temperature observed during the cycle in progress.
    pub peak_gas_temperature_k: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<SimFault>,
}
