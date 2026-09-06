//! The engine configuration catalog.
//!
//! The catalog is data-driven: configurations are parsed from embedded JSON
//! documents, not constructed in code. Only one diesel engine ships today, but
//! list, active-ID, select-by-ID, and reset all go through the same API that a
//! multi-configuration build would use (README "Configuration and provenance").
//!
//! Selecting *any* configuration, including the one already active, performs a
//! documented deterministic reset.

use serde::{Deserialize, Serialize};

use crate::config::{EngineConfig, ProvenanceReport, ValidatedConfig};
use crate::error::{Result, SimError};

/// The built-in OM 471.9 M3D reference configuration document.
///
/// Compiled into the binary so that `web/dist` needs no runtime fetch.
pub const OM471_9_M3D_JSON: &str = include_str!("../data/mercedes-benz-om471-9-m3d-375kw.json");

/// Stable ID of the first built-in engine.
pub const BUILTIN_ENGINE_ID: &str = "mercedes-benz-om471-9-m3d-375kw";

/// Compact catalog entry for selector UIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummary {
    pub id: String,
    pub display_name: String,
    pub manufacturer_reference: String,
    pub power_code: String,
    pub cylinders: usize,
    pub displacement_l: f64,
    pub rated_power_kw: f64,
    pub rated_torque_nm: f64,
    pub idle_rpm: f64,
    pub disclaimer: String,
}

impl ConfigSummary {
    fn from_validated(config: &ValidatedConfig) -> Self {
        let c = config.config();
        Self {
            id: c.identity.id.clone(),
            display_name: c.identity.display_name.clone(),
            manufacturer_reference: c.identity.manufacturer_reference.clone(),
            power_code: c.identity.power_code.clone(),
            cylinders: c.geometry.cylinders,
            displacement_l: config.derived().total_displacement_m3 * 1000.0,
            rated_power_kw: c.rated.max_power_w / 1000.0,
            rated_torque_nm: c.rated.max_torque_nm,
            idle_rpm: c.governor.idle_target_rpm,
            disclaimer: c.identity.disclaimer.clone(),
        }
    }
}

/// A validated set of engine configurations with one active selection.
#[derive(Debug, Clone)]
pub struct Catalog {
    configs: Vec<ValidatedConfig>,
    active: usize,
}

impl Catalog {
    /// Build the catalog from the configurations compiled into this binary.
    pub fn builtin() -> Result<Self> {
        Self::from_documents(&[OM471_9_M3D_JSON])
    }

    /// Build a catalog from JSON documents. Every document is validated; the
    /// first becomes active.
    pub fn from_documents(documents: &[&str]) -> Result<Self> {
        if documents.is_empty() {
            return Err(SimError::invalid_config(
                "a catalog needs at least one configuration",
            ));
        }
        let mut configs = Vec::with_capacity(documents.len());
        for document in documents {
            configs.push(EngineConfig::from_json(document)?.validate()?);
        }
        for (index, config) in configs.iter().enumerate() {
            if configs[..index].iter().any(|c| c.id() == config.id()) {
                return Err(SimError::invalid_config(format!(
                    "duplicate configuration id `{}`",
                    config.id()
                )));
            }
        }
        Ok(Self { configs, active: 0 })
    }

    /// Summaries of every configuration, in catalog order.
    pub fn list(&self) -> Vec<ConfigSummary> {
        self.configs
            .iter()
            .map(ConfigSummary::from_validated)
            .collect()
    }

    /// ID of the active configuration.
    pub fn active_id(&self) -> &str {
        self.configs[self.active].id()
    }

    /// The active configuration.
    pub fn active_config(&self) -> &ValidatedConfig {
        &self.configs[self.active]
    }

    /// Provenance for the active configuration.
    pub fn active_provenance(&self) -> ProvenanceReport {
        self.active_config().provenance_report()
    }

    /// Whether an ID exists in the catalog.
    pub fn contains(&self, id: &str) -> bool {
        self.configs.iter().any(|c| c.id() == id)
    }

    /// Select a configuration by stable ID.
    ///
    /// The caller is expected to follow this with a deterministic reset; see
    /// [`crate::Engine::select_config`], which does both atomically. An unknown
    /// ID leaves the active selection unchanged.
    pub fn select(&mut self, id: &str) -> Result<()> {
        match self.configs.iter().position(|c| c.id() == id) {
            Some(index) => {
                self.active = index;
                Ok(())
            }
            None => Err(SimError::unknown_config_id(id)),
        }
    }
}
