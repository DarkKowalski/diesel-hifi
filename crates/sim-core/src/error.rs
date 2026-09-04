//! Structured errors with stable string codes.
//!
//! The codes cross the WASM boundary verbatim so that the worker and the UI can
//! branch on them without parsing English text.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable machine-readable error codes.
///
/// These strings are part of the public API surface (SPEC section 7, "Return
/// structured errors"). Do not rename an existing variant's string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    /// `Catalog::select` was given an ID that is not in the catalog.
    #[serde(rename = "UNKNOWN_CONFIG_ID")]
    UnknownConfigId,
    /// An `EngineConfig` failed schema, range, or provenance validation.
    #[serde(rename = "INVALID_CONFIG")]
    InvalidConfig,
    /// A control input was non-finite or outside its documented range.
    #[serde(rename = "INVALID_CONTROL")]
    InvalidControl,
    /// Integration produced a non-finite value; the simulation is latched into a fault.
    #[serde(rename = "NON_FINITE_STATE")]
    NonFiniteState,
    /// Cylinder pressure exceeded the configured validation envelope.
    #[serde(rename = "PRESSURE_ENVELOPE_EXCEEDED")]
    PressureEnvelopeExceeded,
    /// A single `advance` call requested more steps than the configuration allows.
    #[serde(rename = "STEP_LIMIT_EXCEEDED")]
    StepLimitExceeded,
    /// An operation was attempted before the handle was initialised.
    #[serde(rename = "NOT_INITIALIZED")]
    NotInitialized,
}

impl ErrorCode {
    /// The stable wire string for this code.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnknownConfigId => "UNKNOWN_CONFIG_ID",
            ErrorCode::InvalidConfig => "INVALID_CONFIG",
            ErrorCode::InvalidControl => "INVALID_CONTROL",
            ErrorCode::NonFiniteState => "NON_FINITE_STATE",
            ErrorCode::PressureEnvelopeExceeded => "PRESSURE_ENVELOPE_EXCEEDED",
            ErrorCode::StepLimitExceeded => "STEP_LIMIT_EXCEEDED",
            ErrorCode::NotInitialized => "NOT_INITIALIZED",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structured simulation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimError {
    pub code: ErrorCode,
    pub message: String,
}

impl SimError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidConfig, message)
    }

    pub fn invalid_control(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidControl, message)
    }

    pub fn unknown_config_id(id: &str) -> Self {
        Self::new(
            ErrorCode::UnknownConfigId,
            format!("no engine configuration with id `{id}`"),
        )
    }
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SimError {}

/// Convenience alias used across the crate.
pub type Result<T> = std::result::Result<T, SimError>;
