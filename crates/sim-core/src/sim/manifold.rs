//! Manifold filling dynamics.
//!
//! Both manifolds are lumped control volumes whose pressure is a state
//! integrated from the imbalance of mass flow in and out:
//!
//! ```text
//! dp/dt = (R * T / V) * (m_dot_in - m_dot_out)
//! ```
//!
//! This is what lets boost be an outcome. Milestone 2 wrote manifold pressure
//! straight from a schedule and needed a first-order lag to break the loop
//! between "how much fuel can we burn" and "how much air is available"; with
//! real filling dynamics that loop is simply not algebraic any more, because
//! pressure is a state that carries over from the previous step.
//!
//! The intake manifold also carries a burned-gas fraction, which is what makes
//! recirculated exhaust visible to the smoke limit: trapped oxygen is the
//! trapped mass less its burned-gas content.

use crate::config::GasProperties;

/// A lumped manifold: pressure, temperature, and burned-gas mass fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct State {
    pub pressure_pa: f64,
    pub temperature_k: f64,
    /// Mass fraction of the contents that is burned gas rather than fresh air.
    pub burned_fraction: f64,
}

impl State {
    /// Density from the ideal-gas law.
    #[inline]
    pub fn density_kg_per_m3(&self, g: &GasProperties) -> f64 {
        self.pressure_pa / (g.gas_constant_j_per_kg_k * self.temperature_k)
    }

    /// Contained mass for a given volume.
    #[inline]
    pub fn mass_kg(&self, g: &GasProperties, volume_m3: f64) -> f64 {
        self.density_kg_per_m3(g) * volume_m3
    }
}

/// Integrate manifold pressure over one step.
///
/// Pressure is floored well above zero: a manifold cannot be pumped to a hard
/// vacuum, and letting it approach zero would make the density and the flow
/// functions that read it blow up.
#[inline]
pub fn advance_pressure(
    g: &GasProperties,
    pressure_pa: f64,
    temperature_k: f64,
    volume_m3: f64,
    net_inflow_kg_per_s: f64,
    dt: f64,
    floor_pa: f64,
) -> f64 {
    let rate = g.gas_constant_j_per_kg_k * temperature_k / volume_m3 * net_inflow_kg_per_s;
    (pressure_pa + rate * dt).max(floor_pa)
}

/// Blend an incoming stream into a manifold's temperature.
///
/// A first-order mixing relaxation whose time constant is the manifold's own
/// residence time: a small manifold with a lot of flow through it takes the
/// incoming temperature quickly, a large one lags.
#[inline]
pub fn mix_temperature(
    current_k: f64,
    incoming_k: f64,
    incoming_flow_kg_per_s: f64,
    contained_mass_kg: f64,
    dt: f64,
) -> f64 {
    if contained_mass_kg <= 0.0 || incoming_flow_kg_per_s <= 0.0 {
        return current_k;
    }
    let blend = (incoming_flow_kg_per_s * dt / contained_mass_kg).clamp(0.0, 1.0);
    current_k + (incoming_k - current_k) * blend
}

/// Blend two incoming streams, one fresh and one burned, into the manifold's
/// burned-gas fraction.
#[inline]
pub fn mix_burned_fraction(
    current: f64,
    fresh_flow_kg_per_s: f64,
    burned_flow_kg_per_s: f64,
    contained_mass_kg: f64,
    dt: f64,
) -> f64 {
    let total = fresh_flow_kg_per_s + burned_flow_kg_per_s;
    if contained_mass_kg <= 0.0 || total <= 0.0 {
        return current;
    }
    let incoming_fraction = burned_flow_kg_per_s / total;
    let blend = (total * dt / contained_mass_kg).clamp(0.0, 1.0);
    (current + (incoming_fraction - current) * blend).clamp(0.0, 1.0)
}

/// Air mass flow drawn by the engine, from the speed-density relation.
///
/// `m_dot = eta_v * V_d * (N / 120) * rho` for a four-stroke, where `N / 120`
/// is intake events per second. Keeping this continuous lets the manifold see a
/// smooth demand while individual cylinders still trap discretely at IVC.
#[inline]
pub fn engine_flow_kg_per_s(
    displacement_m3: f64,
    volumetric_efficiency: f64,
    rpm: f64,
    density_kg_per_m3: f64,
) -> f64 {
    if rpm <= 0.0 {
        return 0.0;
    }
    volumetric_efficiency * displacement_m3 * (rpm / 120.0) * density_kg_per_m3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gas() -> GasProperties {
        GasProperties {
            gas_constant_j_per_kg_k: 287.0,
            cv_reference_j_per_kg_k: 718.0,
            cv_slope_j_per_kg_k2: 0.18,
            reference_temperature_k: 300.0,
        }
    }

    #[test]
    fn net_inflow_raises_pressure_and_net_outflow_lowers_it() {
        let g = gas();
        let up = advance_pressure(&g, 200_000.0, 320.0, 0.02, 0.05, 1.0e-3, 1_000.0);
        let down = advance_pressure(&g, 200_000.0, 320.0, 0.02, -0.05, 1.0e-3, 1_000.0);
        assert!(up > 200_000.0);
        assert!(down < 200_000.0);
    }

    #[test]
    fn pressure_never_falls_through_its_floor() {
        let g = gas();
        let p = advance_pressure(&g, 50_000.0, 320.0, 0.02, -1000.0, 1.0e-3, 1_000.0);
        assert!(p >= 1_000.0, "got {p}");
    }

    #[test]
    fn a_smaller_manifold_responds_faster() {
        let g = gas();
        let small = advance_pressure(&g, 200_000.0, 320.0, 0.005, 0.05, 1.0e-3, 1_000.0);
        let large = advance_pressure(&g, 200_000.0, 320.0, 0.05, 0.05, 1.0e-3, 1_000.0);
        assert!(small > large, "stiffness should scale with 1/volume");
    }

    #[test]
    fn mixing_moves_temperature_toward_the_incoming_stream_without_overshoot() {
        let mut t = 300.0;
        for _ in 0..10_000 {
            t = mix_temperature(t, 450.0, 0.1, 0.02, 1.0e-3);
            assert!((300.0..=450.0).contains(&t), "overshot to {t}");
        }
        assert!(
            (t - 450.0).abs() < 1.0,
            "should approach the source, got {t}"
        );
    }

    #[test]
    fn mixing_is_inert_without_flow_or_mass() {
        assert_eq!(mix_temperature(320.0, 900.0, 0.0, 0.02, 1.0e-3), 320.0);
        assert_eq!(mix_temperature(320.0, 900.0, 0.1, 0.0, 1.0e-3), 320.0);
    }

    #[test]
    fn burned_fraction_settles_at_the_incoming_ratio() {
        let mut f = 0.0;
        for _ in 0..20_000 {
            f = mix_burned_fraction(f, 0.8, 0.2, 0.02, 1.0e-3);
            assert!((0.0..=1.0).contains(&f));
        }
        assert!(
            (f - 0.2).abs() < 0.01,
            "one part burned to four fresh should settle near 0.2, got {f}"
        );
    }

    #[test]
    fn a_stopped_engine_draws_no_air() {
        assert_eq!(engine_flow_kg_per_s(0.0128, 0.92, 0.0, 1.2), 0.0);
    }

    #[test]
    fn engine_flow_scales_with_speed_and_density() {
        let slow = engine_flow_kg_per_s(0.0128, 0.92, 800.0, 1.2);
        let fast = engine_flow_kg_per_s(0.0128, 0.92, 1600.0, 1.2);
        let dense = engine_flow_kg_per_s(0.0128, 0.92, 800.0, 2.4);
        assert!((fast - 2.0 * slow).abs() < 1.0e-12);
        assert!((dense - 2.0 * slow).abs() < 1.0e-12);
    }
}
