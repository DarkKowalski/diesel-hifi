//! Staged decompression engine brake.
//!
//! A decompression brake makes a diesel absorb power by opening an exhaust valve
//! while the cylinder is otherwise closed. The manual describes the mechanism
//! directly, and this module is little more than that description in code.
//!
//! The brake cams carry **two peaks**:
//!
//! ```text
//!    IVC        charge lobe                     release lobe       EVO
//!     |             /\                               /\            |
//!  ---+------------/  \-----------------------------/  \-----------+---
//!  -160 deg     -130 deg                        -12 deg          +140 deg
//!               (fill it)          firing TDC   (dump it)
//! ```
//!
//! Both lobes are narrow, and each is narrow for its own reason. Widening the
//! charging lobe *lowers* absorbed power, because a lobe still open once the
//! piston is rising lets the charge straight back out. Widening the release lobe
//! starts the dump part way up the compression stroke instead of at the top of
//! it, which reaches a similar absorbed power by a different mechanism — one
//! whose peak cylinder pressure sits below the motored trace, contradicting the
//! manual's "the compression pressure is increased as a result".
//!
//! The **charging** lobe opens early in the compression stroke, when the cylinder
//! is near its largest volume and its pressure is below the boosted exhaust
//! manifold. Gas therefore flows *inwards*: the manual says "exhaust flows out of
//! the exhaust manifold back into the cylinder due to the head pressure. The
//! compression pressure is increased as a result and the upward moving piston is
//! braked in its compression stroke."
//!
//! The **release** lobe opens again just before firing TDC, at the top of that
//! now-expensive compression: "part of the compression pressure is reduced. As a
//! result the piston is less rapidly accelerated in the direction of the BDC."
//!
//! That asymmetry is the entire brake. The piston pays full price to compress a
//! deliberately over-filled cylinder and is handed back only what is left after
//! the dump. Nothing in this module computes a braking torque — it computes an
//! open area, the gas flows through it, and
//! [`crate::sim::torque::cylinder_torque_nm`] turns the resulting pressure into
//! crank torque exactly as it does for a firing cylinder.
//!
//! The published brake-power anchors are deliberately not visible from here. They
//! are a validation target, and a model permitted to read its own answer would
//! demonstrate nothing.
//!
//! # Stages
//!
//! | Stage | Manual | Here |
//! |---|---|---|
//! | I | "the brake power is achieved by cylinders 1...3" | the first `stage1_cylinder_count` cylinders |
//! | II | "the brake power is provided by all cylinders" | every cylinder |
//! | III | all cylinders "and through an additional increase in the internal pressure of the cylinder" | every cylinder, plus the wastegate and EGR commands |
//!
//! Stage III's extra pressure comes from the MCM actuating the wastegate and the
//! EGR positioner, which here means overriding the boost setpoint and the EGR
//! valve position. Those overrides carry one value per stage, because the
//! high-performance variant differs from the standard one by raising cylinder
//! pressure "in all engine brake stages" — a data record, not a mechanism.
//!
//! The wastegate loop is also detuned while braking, by
//! `engine_brake.wastegate_gain_scale`. See that field for why: the brake changes
//! the plant the controller is driving by about an order of magnitude, and gains
//! calibrated for the fuelled engine hunt against it.

use crate::config::EngineBrake;

use super::acoustics::ramp;

/// The highest stage the hardware offers.
pub const MAX_STAGE: u8 = 3;

/// What the brake asks of the engine this step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Command {
    /// Whether the brake is actually operating.
    pub active: bool,
    /// The stage in force, `0` when inactive.
    pub stage: u8,
    /// How many cylinders, counted from the first, are braked.
    pub braked_cylinders: usize,
    /// Boost setpoint the MCM regulates to while braking, absolute pascals.
    pub boost_target_pa: f64,
    /// Factor to apply to the wastegate controller's gains and slew while braking.
    pub wastegate_gain_scale: f64,
    /// EGR valve position the MCM commands while braking.
    pub egr_command: f64,
}

impl Command {
    /// Whether a given cylinder index is braked this step.
    #[inline]
    pub fn brakes_cylinder(&self, index: usize) -> bool {
        self.active && index < self.braked_cylinders
    }
}

/// Resolve the brake stage against the published operating conditions.
///
/// The manual permits the brake when the vehicle is in deceleration mode with the
/// drive and clutch pedals released, above 1000 rpm, and with ABS not in
/// closed-loop operation. Two of those three are represented: a released pedal,
/// and the speed floor. This model carries no clutch position and no ABS, so
/// those conditions are simply absent rather than approximated — noted here
/// because a silently dropped precondition is how a model quietly stops matching
/// the thing it claims to model.
pub fn command(
    brake: &EngineBrake,
    cylinder_count: usize,
    stage: u8,
    pedal: f64,
    rpm: f64,
) -> Command {
    let stage = stage.min(MAX_STAGE);
    if stage == 0 || pedal > 0.0 || rpm <= brake.min_speed_rpm {
        return Command::default();
    }

    let braked_cylinders = if stage == 1 {
        (brake.stage1_cylinder_count as usize).min(cylinder_count)
    } else {
        cylinder_count
    };

    let (boost_target_pa, egr_command) = match stage {
        1 => (brake.stage1_boost_target_pa, brake.stage1_egr_command),
        2 => (brake.stage2_boost_target_pa, brake.stage2_egr_command),
        _ => (brake.stage3_boost_target_pa, brake.stage3_egr_command),
    };

    Command {
        active: true,
        stage,
        braked_cylinders,
        boost_target_pa,
        wastegate_gain_scale: brake.wastegate_gain_scale,
        egr_command,
    }
}

/// One brake lobe: a smooth bump peaking at `center` and vanishing `width`
/// either side of it.
///
/// The same raised-cosine shape the exhaust port ramp uses, so the area is
/// continuous at both ends of the event and the transfer never steps.
#[inline]
fn lobe(psi: f64, center: f64, width: f64) -> f64 {
    if width <= 0.0 {
        return 0.0;
    }
    let distance = (psi - center).abs();
    if distance >= width {
        return 0.0;
    }
    ramp(1.0 - distance / width)
}

/// Open area of the braked exhaust valve at a signed cycle angle, m^2.
///
/// The two lobes are combined with `max` rather than a sum: they are the same
/// physical valve opening twice, so where they meet the valve is as open as the
/// nearer peak demands, never twice as open. Validation keeps them from
/// overlapping in the fitted configuration anyway, and this makes the property
/// hold for any configuration.
#[inline]
pub fn port_area_m2(brake: &EngineBrake, psi: f64) -> f64 {
    let charge = lobe(psi, brake.charge_center_rad, brake.charge_width_rad);
    let release = lobe(psi, brake.release_center_rad, brake.release_width_rad);
    brake.effective_area_m2 * charge.max(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brake() -> EngineBrake {
        EngineBrake {
            variant: "M5U".to_string(),
            min_speed_rpm: 1000.0,
            stage1_cylinder_count: 3,
            anchor_low_rpm: 1300.0,
            anchor_low_power_w: 100_000.0,
            anchor_high_rpm: 2300.0,
            anchor_high_power_w: 300_000.0,
            charge_center_rad: -2.53,
            release_center_rad: -0.31,
            charge_width_rad: 0.30,
            release_width_rad: 0.30,
            effective_area_m2: 4.0e-4,
            stage1_boost_target_pa: 101_325.0,
            stage2_boost_target_pa: 101_325.0,
            stage3_boost_target_pa: 320_000.0,
            wastegate_gain_scale: 0.05,
            stage1_egr_command: 0.0,
            stage2_egr_command: 0.0,
            stage3_egr_command: 0.5,
        }
    }

    #[test]
    fn stage_zero_is_inert() {
        let c = command(&brake(), 6, 0, 0.0, 1500.0);
        assert!(!c.active);
        assert_eq!(c.braked_cylinders, 0);
    }

    #[test]
    fn the_published_speed_floor_is_honoured() {
        let b = brake();
        assert!(!command(&b, 6, 3, 0.0, 999.0).active);
        assert!(!command(&b, 6, 3, 0.0, b.min_speed_rpm).active);
        assert!(command(&b, 6, 3, 0.0, 1001.0).active);
    }

    #[test]
    fn touching_the_pedal_releases_the_brake() {
        let b = brake();
        assert!(command(&b, 6, 3, 0.0, 1500.0).active);
        assert!(!command(&b, 6, 3, 0.01, 1500.0).active);
    }

    #[test]
    fn stage_one_brakes_the_published_three_cylinders_and_the_rest_brake_all() {
        let b = brake();
        assert_eq!(command(&b, 6, 1, 0.0, 1500.0).braked_cylinders, 3);
        assert_eq!(command(&b, 6, 2, 0.0, 1500.0).braked_cylinders, 6);
        assert_eq!(command(&b, 6, 3, 0.0, 1500.0).braked_cylinders, 6);
    }

    #[test]
    fn a_smaller_engine_cannot_brake_more_cylinders_than_it_has() {
        let b = brake();
        assert_eq!(command(&b, 2, 1, 0.0, 1500.0).braked_cylinders, 2);
    }

    #[test]
    fn only_the_top_stage_leans_on_the_air_path_in_the_standard_variant() {
        let b = brake();
        assert_eq!(
            command(&b, 6, 1, 0.0, 1500.0).boost_target_pa,
            b.stage1_boost_target_pa
        );
        // Stage III asks the wastegate for more boost than stage II, which is
        // what raises cylinder pressure and makes it the stronger stage.
        assert!(
            command(&b, 6, 3, 0.0, 1500.0).boost_target_pa
                > command(&b, 6, 2, 0.0, 1500.0).boost_target_pa
        );
        assert!(command(&b, 6, 3, 0.0, 1500.0).egr_command > 0.0);
    }

    #[test]
    fn the_cam_has_exactly_two_peaks_and_is_shut_between_them() {
        let b = brake();
        assert!(port_area_m2(&b, b.charge_center_rad) > 0.0);
        assert!(port_area_m2(&b, b.release_center_rad) > 0.0);

        // Halfway between the lobes the valve is fully shut.
        let midpoint = 0.5 * (b.charge_center_rad + b.release_center_rad);
        assert_eq!(port_area_m2(&b, midpoint), 0.0);

        // And shut well away from either.
        assert_eq!(port_area_m2(&b, 1.5), 0.0);
        assert_eq!(port_area_m2(&b, -3.0), 0.0);
    }

    #[test]
    fn each_lobe_peaks_at_exactly_the_valve_area() {
        let b = brake();
        for center in [b.charge_center_rad, b.release_center_rad] {
            let peak = port_area_m2(&b, center);
            assert!(
                (peak - b.effective_area_m2).abs() < 1.0e-12,
                "lobe at {center} peaked at {peak}, not {}",
                b.effective_area_m2
            );
        }
    }

    #[test]
    fn the_area_is_continuous_where_each_lobe_begins_and_ends() {
        let b = brake();
        for center in [b.charge_center_rad, b.release_center_rad] {
            for edge in [center - b.charge_width_rad, center + b.charge_width_rad] {
                assert!(port_area_m2(&b, edge).abs() < 1.0e-12);
                // Just inside the edge the area is small but non-zero: it eases
                // in rather than stepping.
                let inside = port_area_m2(&b, edge + 1.0e-4 * (center - edge).signum());
                assert!(inside > 0.0 && inside < b.effective_area_m2 * 0.01);
            }
        }
    }

    #[test]
    fn overlapping_lobes_never_open_the_valve_further_than_it_goes() {
        // Validation forbids this in a fitted configuration, but the shape
        // function must hold regardless of what it is handed.
        let mut b = brake();
        b.release_center_rad = b.charge_center_rad + 0.05;
        b.charge_width_rad = 0.5;
        b.release_width_rad = 0.5;
        for i in 0..2000 {
            let psi = -3.0 + f64::from(i) * 0.003;
            assert!(port_area_m2(&b, psi) <= b.effective_area_m2 + 1.0e-12);
        }
    }
}
