//! Deterministic diesel truck engine simulation core.
//!
//! This crate is platform independent: it touches no browser, DOM, audio-device,
//! or filesystem API. SI units and radians are used throughout, and one
//! four-stroke cycle spans `4 * PI` radians.
//!
//! The reference engine is a public-data model of the 2011 Mercedes-Benz
//! OM 471.9 M3D. It is **not** an OEM-certified digital twin, and every value
//! carries provenance saying whether it was published, derived, or calibrated.
//!
//! # Layering
//!
//! - [`config`] holds the versioned, validated [`config::EngineConfig`].
//! - [`catalog::Catalog`] exposes list / active-ID / select-by-ID.
//! - [`sim::Simulation`] integrates the physics at a fixed step.
//! - [`Engine`] binds a catalog to a simulation and is what adapters wrap.
//!
//! Solver code never branches on an engine ID; every engine-specific number
//! arrives through [`config::ValidatedConfig`].

pub mod catalog;
pub mod config;
pub mod dyno;
pub mod error;
pub mod geometry;
pub mod rng;
pub mod sim;
pub mod snapshot;

pub use catalog::{Catalog, ConfigSummary, BUILTIN_ENGINE_ID};
pub use config::{EngineConfig, ProvenanceReport, ProvenanceStatus, ValidatedConfig};
pub use dyno::{OperatingPoint, SweepOptions, SweepPeaks};
pub use error::{ErrorCode, Result, SimError};
pub use sim::{Controls, CycleAverages, ResetOptions, Simulation};
pub use snapshot::{RunState, Snapshot};

/// Version of the public engine API. Bump on a breaking change.
pub const API_VERSION: u32 = 2;

/// A catalog bound to a running simulation.
///
/// This is the whole surface an adapter needs: the WASM crate is a thin
/// translation layer over these methods and holds no physics of its own.
#[derive(Debug, Clone)]
pub struct Engine {
    catalog: Catalog,
    simulation: Simulation,
    /// The options used for the most recent explicit reset. Reused when a
    /// configuration is selected, so selection is reproducible.
    reset_options: ResetOptions,
}

impl Engine {
    /// Build an engine over the configurations compiled into this binary.
    pub fn builtin() -> Result<Self> {
        Self::from_catalog(Catalog::builtin()?, ResetOptions::default())
    }

    /// Build an engine over a supplied catalog.
    pub fn from_catalog(catalog: Catalog, reset_options: ResetOptions) -> Result<Self> {
        let simulation = Simulation::new(catalog.active_config().clone(), reset_options)?;
        Ok(Self {
            catalog,
            simulation,
            reset_options,
        })
    }

    /// Summaries of every available configuration.
    pub fn list_configs(&self) -> Vec<ConfigSummary> {
        self.catalog.list()
    }

    /// Stable ID of the active configuration.
    pub fn active_config_id(&self) -> &str {
        self.catalog.active_id()
    }

    /// Provenance for the active configuration, including source records.
    pub fn provenance(&self) -> ProvenanceReport {
        self.catalog.active_provenance()
    }

    /// Select a configuration by stable ID.
    ///
    /// Selecting **any** configuration, including the one already active,
    /// performs a deterministic reset using the options from the most recent
    /// explicit [`Engine::reset`] (or the defaults, if none). An unknown ID
    /// returns [`ErrorCode::UnknownConfigId`] and changes nothing.
    pub fn select_config(&mut self, id: &str) -> Result<()> {
        self.catalog.select(id)?;
        self.simulation =
            Simulation::new(self.catalog.active_config().clone(), self.reset_options)?;
        Ok(())
    }

    /// Reset with explicit seed and initial conditions.
    pub fn reset(&mut self, options: ResetOptions) -> Result<()> {
        self.simulation.reset(options)?;
        self.reset_options = options;
        Ok(())
    }

    /// Submit control inputs.
    pub fn set_controls(&mut self, controls: Controls) -> Result<()> {
        self.simulation.set_controls(controls)
    }

    /// Advance `steps` fixed steps and return the resulting snapshot.
    pub fn advance(&mut self, steps: u32) -> Result<Snapshot> {
        self.simulation.advance(steps)?;
        Ok(self.simulation.snapshot())
    }

    /// Telemetry without advancing.
    pub fn snapshot(&self) -> Snapshot {
        self.simulation.snapshot()
    }

    /// Fixed integration step of the active configuration, in seconds.
    pub fn fixed_step_s(&self) -> f64 {
        self.simulation.config().config().solver.fixed_step_s
    }

    /// Largest batch the active configuration permits in one `advance` call.
    pub fn max_steps_per_batch(&self) -> u32 {
        self.simulation.config().config().solver.max_steps_per_batch
    }

    /// Read-only access to the simulation, for tests and diagnostics.
    pub fn simulation(&self) -> &Simulation {
        &self.simulation
    }

    /// Cycle-averaged results of the most recently completed four-stroke cycle.
    pub fn cycle_averages(&self) -> CycleAverages {
        self.simulation.cycle_averages()
    }

    /// Measure one steady-state operating point on a separate simulation
    /// instance, leaving the live one untouched.
    pub fn operating_point(
        &self,
        rpm: f64,
        pedal: f64,
        settle_cycles: u32,
        measure_cycles: u32,
    ) -> Result<OperatingPoint> {
        dyno::operating_point(
            self.catalog.active_config(),
            rpm,
            pedal,
            settle_cycles,
            measure_cycles,
        )
    }

    /// Measure a speed sweep of the active configuration.
    ///
    /// The speeds at which peak power and peak torque land are an outcome of
    /// this project's calibration, not published OEM data.
    pub fn sweep(&self, options: SweepOptions) -> Result<Vec<OperatingPoint>> {
        dyno::sweep(self.catalog.active_config(), options)
    }
}
