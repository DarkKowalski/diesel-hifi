//! Per-parameter provenance and source records.
//!
//! README "Configuration and provenance": every configuration parameter must be
//! labelled `published`,
//! `derived`, or `calibrated`. Published values must name a source record and a
//! locator. Derived values must state their formula and inputs. Calibrated
//! values must state their purpose and safe range.
//!
//! Nothing in this module is engine-specific; the rules are enforced uniformly
//! by [`crate::config::validate`].

use serde::{Deserialize, Serialize};

/// How a configuration value was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceStatus {
    /// Printed verbatim in a named source document.
    Published,
    /// Computed from published values by a stated formula.
    Derived,
    /// Chosen by us to make the model run. Never OEM data.
    Calibrated,
}

impl ProvenanceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProvenanceStatus::Published => "published",
            ProvenanceStatus::Derived => "derived",
            ProvenanceStatus::Calibrated => "calibrated",
        }
    }
}

/// A citable document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    /// Stable key referenced by [`ProvenanceEntry::source_id`].
    pub id: String,
    pub title: String,
    pub publisher: String,
    /// Technical status date of the document, ISO-8601.
    pub technical_status: String,
    /// Publication / order number.
    pub order_number: String,
    /// Which engine variants and subsystems the document actually covers.
    pub scope: String,
    /// Optional path to a local copy. Never fetched at runtime.
    #[serde(default)]
    pub local_path: Option<String>,
}

/// Provenance for exactly one configuration parameter path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceEntry {
    /// Dotted path into [`crate::config::EngineConfig`], e.g. `geometry.bore_m`.
    pub path: String,
    pub status: ProvenanceStatus,
    /// Human-readable rendering of the value as it appears in the source or as
    /// we chose it, e.g. `"132 mm"`.
    pub value_note: String,

    /// Required when `status == Published`.
    #[serde(default)]
    pub source_id: Option<String>,
    /// Required when `status == Published`. Document number / section / page.
    #[serde(default)]
    pub locator: Option<String>,

    /// Required when `status == Derived`.
    #[serde(default)]
    pub formula: Option<String>,
    /// Required (non-empty) when `status == Derived`.
    #[serde(default)]
    pub inputs: Vec<String>,

    /// Required when `status == Calibrated`.
    #[serde(default)]
    pub purpose: Option<String>,
    /// Required when `status == Calibrated`. Must contain the configured value.
    #[serde(default)]
    pub safe_range: Option<[f64; 2]>,
}

impl ProvenanceEntry {
    pub fn is_published(&self) -> bool {
        self.status == ProvenanceStatus::Published
    }
    pub fn is_derived(&self) -> bool {
        self.status == ProvenanceStatus::Derived
    }
    pub fn is_calibrated(&self) -> bool {
        self.status == ProvenanceStatus::Calibrated
    }
}

/// The provenance view handed to the UI so that estimates are never presented
/// as OEM data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceReport {
    pub config_id: String,
    pub display_name: String,
    pub disclaimer: String,
    pub sources: Vec<SourceRecord>,
    pub entries: Vec<ProvenanceEntry>,
    pub published_count: usize,
    pub derived_count: usize,
    pub calibrated_count: usize,
}
