//! Slider-crank geometry.
//!
//! All angles are radians measured from a piston top dead centre, with period
//! `2 * PI`. Cylinder volume and its angular derivative come from here; the
//! solver uses `dV/dtheta` directly to turn cylinder pressure into crank torque.

use crate::config::validate::DerivedGeometry;

/// Precomputed slider-crank constants for one cylinder bore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderCrank {
    /// Crank radius, `stroke / 2`.
    pub crank_radius_m: f64,
    /// Connecting-rod length between centres.
    pub connecting_rod_m: f64,
    /// Piston crown area.
    pub piston_area_m2: f64,
    /// Volume at top dead centre.
    pub clearance_volume_m3: f64,
}

impl SliderCrank {
    pub fn new(
        crank_radius_m: f64,
        connecting_rod_m: f64,
        piston_area_m2: f64,
        clearance_volume_m3: f64,
    ) -> Self {
        Self {
            crank_radius_m,
            connecting_rod_m,
            piston_area_m2,
            clearance_volume_m3,
        }
    }

    /// Build from precomputed derived geometry plus the connecting-rod length.
    pub fn from_derived(derived: &DerivedGeometry, connecting_rod_m: f64) -> Self {
        Self::new(
            derived.crank_radius_m,
            connecting_rod_m,
            derived.piston_area_m2,
            derived.clearance_volume_m3,
        )
    }

    /// `sqrt(l^2 - a^2 sin^2(theta))`, the rod's axial projection.
    #[inline]
    fn rod_projection(&self, sin_theta: f64) -> f64 {
        let a_sin = self.crank_radius_m * sin_theta;
        (self.connecting_rod_m * self.connecting_rod_m - a_sin * a_sin).sqrt()
    }

    /// Piston displacement below top dead centre, in metres.
    ///
    /// `x(theta) = a(1 - cos theta) + l - sqrt(l^2 - a^2 sin^2 theta)`
    #[inline]
    pub fn piston_displacement_m(&self, theta: f64) -> f64 {
        let (sin_theta, cos_theta) = theta.sin_cos();
        self.crank_radius_m * (1.0 - cos_theta) + self.connecting_rod_m
            - self.rod_projection(sin_theta)
    }

    /// Instantaneous cylinder volume.
    #[inline]
    pub fn volume_m3(&self, theta: f64) -> f64 {
        self.clearance_volume_m3 + self.piston_area_m2 * self.piston_displacement_m(theta)
    }

    /// `dV/dtheta`. Positive while the piston descends.
    #[inline]
    pub fn dvolume_dtheta_m3_per_rad(&self, theta: f64) -> f64 {
        let (sin_theta, cos_theta) = theta.sin_cos();
        let a = self.crank_radius_m;
        let projection = self.rod_projection(sin_theta);
        let dx = a * sin_theta * (1.0 + (a * cos_theta) / projection);
        self.piston_area_m2 * dx
    }

    /// Mean piston speed for a given crankshaft angular velocity.
    #[inline]
    pub fn mean_piston_speed_m_per_s(&self, omega_rad_per_s: f64) -> f64 {
        // Mean piston speed = 2 * stroke * revolutions per second.
        2.0 * (2.0 * self.crank_radius_m) * (omega_rad_per_s.abs() / (2.0 * std::f64::consts::PI))
    }
}

/// Total swept volume for `cylinders` bores of the given geometry.
pub fn total_displacement_m3(piston_area_m2: f64, stroke_m: f64, cylinders: usize) -> f64 {
    piston_area_m2 * stroke_m * cylinders as f64
}
