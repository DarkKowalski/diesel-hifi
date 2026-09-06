//! One-dimensional interpolated breakpoint tables.
//!
//! Used for calibrated schedules such as full-load boost pressure and start of
//! injection against engine speed. Lookups are clamped at both ends, allocate
//! nothing, and are pure functions of their input, so they are safe in the hot
//! loop and preserve determinism.

use serde::{Deserialize, Serialize};

use crate::error::{Result, SimError};

/// A monotonically increasing breakpoint table with linear interpolation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// Strictly increasing x values.
    pub breakpoints: Vec<f64>,
    /// One y value per breakpoint.
    pub values: Vec<f64>,
}

impl Schedule {
    pub fn new(breakpoints: Vec<f64>, values: Vec<f64>) -> Self {
        Self {
            breakpoints,
            values,
        }
    }

    /// Check shape, finiteness, and strict monotonicity of the breakpoints.
    pub fn validate(&self, path: &str) -> Result<()> {
        if self.breakpoints.len() < 2 {
            return Err(SimError::invalid_config(format!(
                "`{path}` needs at least two breakpoints"
            )));
        }
        if self.breakpoints.len() != self.values.len() {
            return Err(SimError::invalid_config(format!(
                "`{path}` has {} breakpoints but {} values",
                self.breakpoints.len(),
                self.values.len()
            )));
        }
        for (index, x) in self.breakpoints.iter().enumerate() {
            if !x.is_finite() {
                return Err(SimError::invalid_config(format!(
                    "`{path}` breakpoint {index} is not finite"
                )));
            }
            if index > 0 && *x <= self.breakpoints[index - 1] {
                return Err(SimError::invalid_config(format!(
                    "`{path}` breakpoints must strictly increase, but {x} follows {}",
                    self.breakpoints[index - 1]
                )));
            }
        }
        for (index, y) in self.values.iter().enumerate() {
            if !y.is_finite() {
                return Err(SimError::invalid_config(format!(
                    "`{path}` value {index} is not finite"
                )));
            }
        }
        Ok(())
    }

    /// Smallest configured value.
    pub fn min_value(&self) -> f64 {
        self.values.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// Largest configured value.
    pub fn max_value(&self) -> f64 {
        self.values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Linear interpolation, clamped outside the breakpoint range.
    ///
    /// Assumes [`Schedule::validate`] has already passed; an empty table returns
    /// zero rather than panicking.
    #[inline]
    pub fn lookup(&self, x: f64) -> f64 {
        let n = self.breakpoints.len();
        if n == 0 || self.values.len() != n {
            return 0.0;
        }
        if x <= self.breakpoints[0] {
            return self.values[0];
        }
        if x >= self.breakpoints[n - 1] {
            return self.values[n - 1];
        }
        // Linear scan: these tables have a handful of entries, so this is
        // cheaper and more predictable than a binary search.
        for index in 1..n {
            let upper = self.breakpoints[index];
            if x <= upper {
                let lower = self.breakpoints[index - 1];
                let span = upper - lower;
                let t = if span > 0.0 { (x - lower) / span } else { 0.0 };
                return self.values[index - 1] + t * (self.values[index] - self.values[index - 1]);
            }
        }
        self.values[n - 1]
    }
}
