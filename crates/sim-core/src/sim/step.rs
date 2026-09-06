//! The fixed-step integrator.
//!
//! One call advances the crank by `omega * dt`, updates the air path and every
//! cylinder's gas state over that angle, sums the explicit torque terms, and
//! integrates crank speed. Crank torque is produced only by cylinder pressure
//! acting through slider-crank geometry, plus the starter; no term writes a
//! target torque into crank acceleration.
//!
//! The loop allocates nothing, logs nothing, and performs no I/O.

use crate::config::validate::{ValidatedConfig, CYCLE_RAD};
use crate::error::{ErrorCode, Result, SimError};
use crate::geometry::SliderCrank;
use crate::sim::cylinder::{self, Phase};
use crate::sim::governor::{self, rpm_to_rad_per_s};
use crate::sim::{
    gas, heat_release, heat_transfer, ignition_delay, injection, torque, wrap_cycle, SimState,
    StepReport,
};

/// Pascals per bar, used by the boost-referenced calibration coefficients.
const PA_PER_BAR: f64 = 1.0e5;

pub(super) fn step(
    config: &ValidatedConfig,
    slider: &SliderCrank,
    state: &mut SimState,
) -> Result<()> {
    let cfg = config.config();
    let derived = config.derived();

    let dt = cfg.solver.fixed_step_s;
    let gas_props = &cfg.gas;
    let air = &cfg.air_path;
    let combustion = &cfg.combustion;
    let inj = &cfg.injection;
    let valvetrain = &cfg.valvetrain;
    let limits = &cfg.limits;
    let bore_m = cfg.geometry.bore_m;
    let count = state.cylinder_count;

    // --- fuelling: pedal and idle governor both request fuel, larger wins ---
    let rpm = governor::rad_per_s_to_rpm(state.omega_rad_per_s);
    let idle_request_mg = state
        .governor
        .update(&cfg.governor, state.omega_rad_per_s, dt);
    let demand_mg = governor::fuel_demand_mg(
        inj,
        &cfg.governor,
        state.controls.pedal,
        idle_request_mg,
        state.controls.ignition,
        rpm,
    );
    state.fuel_demand_mg = demand_mg;

    // --- air path ---
    //
    // Milestone 2 prescribes charge pressure from a calibrated full-load
    // schedule scaled by fuelling demand, relaxed through a first-order lag that
    // stands in for turbocharger inertia. Milestone 3 replaces the source of
    // these three values with wastegate turbo dynamics; nothing downstream
    // changes.
    let ambient_pa = air.ambient_pressure_pa;
    let load_fraction = (demand_mg / inj.max_fuel_mg_per_cycle).clamp(0.0, 1.0);
    let target_pa =
        ambient_pa + (air.boost_target_schedule.lookup(rpm) - ambient_pa) * load_fraction;
    state.manifold_pressure_pa +=
        (target_pa - state.manifold_pressure_pa) * (dt / air.boost_response_time_s);
    let boost_bar = ((state.manifold_pressure_pa - ambient_pa) / PA_PER_BAR).max(0.0);
    state.manifold_temperature_k =
        air.charge_temperature_base_k + air.charge_temperature_per_bar_k * boost_bar;
    state.exhaust_pressure_pa =
        air.exhaust_manifold_pressure_pa + air.exhaust_pressure_per_bar_boost * boost_bar;

    let manifold_pa = state.manifold_pressure_pa;
    let manifold_k = state.manifold_temperature_k;
    let exhaust_pa = state.exhaust_pressure_pa;

    // --- crank angle ---
    let theta_old = state.crank_angle_rad;
    let d_theta = state.omega_rad_per_s * dt;
    let theta_unwrapped = theta_old + d_theta;
    let theta_new = wrap_cycle(theta_unwrapped);
    let cycle_completed = theta_unwrapped >= CYCLE_RAD;
    if cycle_completed {
        state.peak_pressure_pa_prev_cycle = state.peak_pressure_pa_cycle;
        state.peak_pressure_pa_cycle = 0.0;
        state.peak_temperature_k_prev_cycle = state.peak_temperature_k_cycle;
        state.peak_temperature_k_cycle = 0.0;
    }

    let piston_speed_m_per_s = slider.mean_piston_speed_m_per_s(state.omega_rad_per_s);

    // --- cylinder gas states and pressure-derived torque ---
    let mut torque_gas_nm = 0.0;
    let mut torque_pumping_nm = 0.0;
    let mut step_peak_pressure_pa: f64 = 0.0;
    let mut step_peak_temperature_k: f64 = 0.0;
    let mut fuel_latched_kg = 0.0;

    for index in 0..count {
        let mut c = state.cylinders[index];
        let psi_old = cylinder::signed_cycle_angle(theta_old, c.phase_offset_rad);
        let psi_new = cylinder::signed_cycle_angle(theta_new, c.phase_offset_rad);
        let phase_new = cylinder::phase_at(
            psi_new,
            valvetrain.intake_valve_close_rad,
            valvetrain.exhaust_valve_open_rad,
        );
        let volume_old_m3 = slider.volume_m3(psi_old);
        let volume_new_m3 = slider.volume_m3(psi_new);

        if phase_new == Phase::Closed && c.phase != Phase::Closed {
            // --- intake valve closing ---
            //
            // Trap the fresh charge on top of whatever burned gas survived the
            // exhaust stroke, then schedule this cycle's injection. The smoke
            // limit sees fresh air only, so residual correctly reduces the
            // oxygen available.
            let ideal_charge_kg = gas::mass_kg(gas_props, manifold_pa, manifold_k, volume_new_m3)
                * air.volumetric_efficiency;
            let residual_kg = c.residual_kg.max(0.0);
            let fresh_kg = (ideal_charge_kg - residual_kg).max(0.0);
            let total_kg = residual_kg + fresh_kg;
            let mixed_k = if total_kg > 0.0 {
                (residual_kg * c.residual_temperature_k + fresh_kg * manifold_k) / total_kg
            } else {
                manifold_k
            };

            c.mass_kg = total_kg;
            c.trapped_air_kg = fresh_kg;
            c.temperature_k = mixed_k;
            c.pressure_pa = gas::pressure_pa(gas_props, total_kg, mixed_k, volume_new_m3);
            c.motored_pressure_pa = c.pressure_pa;
            c.motored_temperature_k = mixed_k;
            c.burned_fraction = 0.0;
            c.wall_heat_loss_j = 0.0;
            c.profile_ready = false;
            c.profile = heat_release::Profile::NONE;

            let smoke_limit_kg = fresh_kg / inj.smoke_limit_afr;
            let fuel_kg = (demand_mg * 1.0e-6).min(smoke_limit_kg).max(0.0);
            c.injection =
                injection::schedule(inj, fuel_kg, rpm, state.omega_rad_per_s, c.pressure_pa);

            fuel_latched_kg += fuel_kg;
            state.last_fuel_charge_mg = fuel_kg * 1.0e6;
            state.last_variant = c.injection.variant;
            state.last_injection_pressure_pa = c.injection.injection_pressure_pa;
        } else if phase_new == Phase::Intake && c.phase == Phase::Exhaust {
            // --- TDC overlap: whatever is left is the residual for next cycle ---
            c.residual_kg = c.mass_kg.max(0.0);
            c.residual_temperature_k = c.temperature_k;
            c.temperature_k = manifold_k;
            c.pressure_pa = manifold_pa;
            c.mass_kg = gas::mass_kg(gas_props, manifold_pa, manifold_k, volume_new_m3);
            c.injection = injection::Event::NONE;
            c.profile = heat_release::Profile::NONE;
            c.profile_ready = false;
            c.burned_fraction = 0.0;
        } else {
            match phase_new {
                Phase::Closed => {
                    // Build the burn profile the moment injection begins, using
                    // the cylinder conditions actually present at that angle.
                    let soi = c.injection.start_of_injection_rad;
                    if !c.profile_ready
                        && c.injection.fuel_kg > 0.0
                        && psi_old < soi
                        && psi_new >= soi
                    {
                        let delay_rad = ignition_delay::ignition_delay_rad(
                            combustion.cetane_number,
                            c.pressure_pa,
                            c.temperature_k,
                            piston_speed_m_per_s,
                        );
                        c.profile = heat_release::profile(
                            combustion,
                            &c.injection,
                            delay_rad,
                            inj.fuel_lower_heating_value_j_per_kg,
                        );
                        c.profile_ready = true;
                        state.last_ignition_delay_rad = delay_rad;
                        state.last_premixed_fraction = c.profile.premixed_fraction;
                    }

                    let dv = volume_new_m3 - volume_old_m3;

                    // Motored trace: isentropic, no combustion, no wall loss.
                    let gamma = gas::gamma(gas_props, c.motored_temperature_k);
                    let volume_ratio = if volume_new_m3 > 0.0 {
                        volume_old_m3 / volume_new_m3
                    } else {
                        1.0
                    };
                    c.motored_pressure_pa *= volume_ratio.powf(gamma);
                    c.motored_temperature_k *= volume_ratio.powf(gamma - 1.0);

                    // Injected fuel joins the trapped mass as it is delivered.
                    let fuel_added_kg = (c.injection.delivered_kg(psi_new)
                        - c.injection.delivered_kg(psi_old))
                    .max(0.0);
                    c.mass_kg += fuel_added_kg;

                    let (heat_j, burned_fraction) = if c.profile_ready {
                        heat_release::heat_release_j(
                            combustion,
                            &c.profile,
                            c.burned_fraction,
                            psi_new,
                        )
                    } else {
                        (0.0, c.burned_fraction)
                    };
                    c.burned_fraction = burned_fraction;

                    let velocity = heat_transfer::gas_velocity_m_per_s(
                        &cfg.heat_transfer,
                        true,
                        piston_speed_m_per_s,
                        c.pressure_pa,
                        c.motored_pressure_pa,
                        derived.displacement_per_cylinder_m3,
                        manifold_pa,
                        manifold_k,
                        derived.max_volume_m3,
                    );
                    let heat_loss_j = heat_transfer::heat_loss_j(
                        &cfg.heat_transfer,
                        slider,
                        bore_m,
                        psi_new,
                        c.pressure_pa,
                        c.temperature_k,
                        velocity,
                        state.omega_rad_per_s,
                        d_theta,
                    );
                    c.wall_heat_loss_j += heat_loss_j;

                    // dT = [ dQ_comb - dQ_wall - p dV ] / (m cv(T))
                    let cv = gas::cv_j_per_kg_k(gas_props, c.temperature_k);
                    if c.mass_kg > 0.0 && cv > 0.0 {
                        c.temperature_k +=
                            (heat_j - heat_loss_j - c.pressure_pa * dv) / (c.mass_kg * cv);
                    }
                    c.temperature_k = c.temperature_k.max(1.0);
                    c.pressure_pa =
                        gas::pressure_pa(gas_props, c.mass_kg, c.temperature_k, volume_new_m3);
                }
                Phase::Intake => {
                    c.temperature_k = manifold_k;
                    c.pressure_pa = manifold_pa;
                    c.mass_kg = gas::mass_kg(gas_props, manifold_pa, manifold_k, volume_new_m3);
                    c.motored_pressure_pa = manifold_pa;
                    c.motored_temperature_k = manifold_k;
                    c.burned_fraction = 0.0;
                }
                Phase::Exhaust => {
                    c.pressure_pa = exhaust_pa;
                    c.mass_kg = gas::mass_kg(gas_props, exhaust_pa, c.temperature_k, volume_new_m3);
                    c.motored_pressure_pa = exhaust_pa;
                }
            }
        }
        c.phase = phase_new;

        if !c.pressure_pa.is_finite() || !c.temperature_k.is_finite() || !c.mass_kg.is_finite() {
            return Err(SimError::new(
                ErrorCode::NonFiniteState,
                format!("cylinder {} produced a non-finite gas state", index + 1),
            ));
        }
        if c.pressure_pa > limits.max_combustion_pressure_pa {
            return Err(SimError::new(
                ErrorCode::PressureEnvelopeExceeded,
                format!(
                    "cylinder {} reached {:.2} MPa, above the {:.2} MPa validation envelope",
                    index + 1,
                    c.pressure_pa / 1.0e6,
                    limits.max_combustion_pressure_pa / 1.0e6
                ),
            ));
        }
        if c.temperature_k > limits.max_gas_temperature_k {
            return Err(SimError::new(
                ErrorCode::NonFiniteState,
                format!(
                    "cylinder {} reached {:.0} K, above the {:.0} K limit",
                    index + 1,
                    c.temperature_k,
                    limits.max_gas_temperature_k
                ),
            ));
        }

        let dv_dtheta = slider.dvolume_dtheta_m3_per_rad(psi_new);
        let contribution =
            torque::cylinder_torque_nm(c.pressure_pa, air.crankcase_pressure_pa, dv_dtheta);
        if phase_new == Phase::Closed {
            torque_gas_nm += contribution;
        } else {
            torque_pumping_nm += contribution;
        }

        step_peak_pressure_pa = step_peak_pressure_pa.max(c.pressure_pa);
        step_peak_temperature_k = step_peak_temperature_k.max(c.temperature_k);
        state.cylinders[index] = c;
    }

    state.peak_pressure_pa_cycle = state.peak_pressure_pa_cycle.max(step_peak_pressure_pa);
    state.peak_pressure_pa_session = state.peak_pressure_pa_session.max(step_peak_pressure_pa);
    state.peak_temperature_k_cycle = state.peak_temperature_k_cycle.max(step_peak_temperature_k);

    // --- explicit resisting and driving torque terms ---
    let peak_for_friction_pa = state
        .peak_pressure_pa_cycle
        .max(state.peak_pressure_pa_prev_cycle);
    let torque_friction_nm = torque::friction_torque_nm(
        torque::fmep_pa(&cfg.friction, peak_for_friction_pa, piston_speed_m_per_s),
        derived.total_displacement_m3,
    );
    let torque_accessory_nm = torque::accessory_torque_nm(&cfg.load, state.omega_rad_per_s);
    let torque_starter_nm = torque::starter_torque_nm(&cfg.load, state.controls.starter, rpm);
    let torque_load_nm = state.controls.load_torque_nm;

    let torque_net_nm = torque_gas_nm + torque_pumping_nm + torque_starter_nm
        - torque_friction_nm
        - torque_accessory_nm
        - torque_load_nm;

    state.report = StepReport {
        torque_gas_nm,
        torque_pumping_nm,
        torque_friction_nm,
        torque_accessory_nm,
        torque_starter_nm,
        torque_load_nm,
        torque_net_nm,
    };

    // --- cycle averaging ---
    //
    // Work, not torque, is what integrates cleanly across a cycle whose angular
    // velocity is changing.
    state.cycle.accumulate(&state.report, d_theta, dt);
    state.cycle.add_fuel(fuel_latched_kg);
    if cycle_completed {
        state.cycle.complete(derived.total_displacement_m3);
    }

    // --- crank dynamics ---
    let inertia_kg_m2 = cfg.inertia.rotating_inertia_kg_m2;
    let mut omega = state.omega_rad_per_s + (torque_net_nm / inertia_kg_m2) * dt;
    if !omega.is_finite() {
        return Err(SimError::new(
            ErrorCode::NonFiniteState,
            "crank angular velocity became non-finite",
        ));
    }
    // The crankshaft is not driven backwards in this model.
    if omega < 0.0 {
        omega = 0.0;
    }
    if omega > rpm_to_rad_per_s(limits.max_rpm) {
        return Err(SimError::new(
            ErrorCode::NonFiniteState,
            format!(
                "engine speed {:.0} rpm exceeded the {:.0} rpm hard limit",
                governor::rad_per_s_to_rpm(omega),
                limits.max_rpm
            ),
        ));
    }

    state.omega_rad_per_s = omega;
    state.crank_angle_rad = theta_new;
    state.sim_time_s += dt;
    state.steps_advanced += 1;

    Ok(())
}
