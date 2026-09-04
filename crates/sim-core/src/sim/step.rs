//! The fixed-step integrator.
//!
//! One call advances the crank by `omega * dt`, updates every cylinder's gas
//! state over that angle, sums the explicit torque terms, and integrates crank
//! speed. Crank torque is produced only by cylinder pressure acting through
//! slider-crank geometry, plus the starter; no term writes a target torque into
//! crank acceleration.
//!
//! The loop allocates nothing, logs nothing, and performs no I/O.

use crate::config::validate::CYCLE_RAD;
use crate::error::{ErrorCode, Result, SimError};
use crate::sim::cylinder::{self, Phase};
use crate::sim::governor::{self, rpm_to_rad_per_s};
use crate::sim::{heat_release, torque, wrap_cycle, Simulation, StepReport};

pub(super) fn step(sim: &mut Simulation) -> Result<()> {
    // All configuration sections used here are plain scalar records, copied out
    // once so the hot loop touches no shared borrow.
    let (combustion, friction, air, gov_cfg, load_cfg, injection, valvetrain, limits, solver) = {
        let cfg = sim.config.config();
        (
            cfg.combustion,
            cfg.friction,
            cfg.air_path,
            cfg.governor,
            cfg.load,
            cfg.injection,
            cfg.valvetrain,
            cfg.limits,
            cfg.solver,
        )
    };
    let inertia_kg_m2 = sim.config.config().inertia.rotating_inertia_kg_m2;
    let total_displacement_m3 = sim.config.derived().total_displacement_m3;
    let slider = sim.slider;
    let count = sim.cylinder_count;
    let dt = solver.fixed_step_s;
    let r = air.gas_constant_j_per_kg_k;

    // --- fuelling: pedal and idle governor both request fuel, larger wins ---
    let rpm = sim.rpm();
    let idle_request_mg = sim.governor.update(&gov_cfg, sim.omega_rad_per_s, dt);
    let demand_mg = governor::fuel_demand_mg(
        &injection,
        &gov_cfg,
        sim.controls.pedal,
        idle_request_mg,
        sim.controls.ignition,
        rpm,
    );
    sim.fuel_demand_mg = demand_mg;

    // --- crank angle ---
    let theta_old = sim.crank_angle_rad;
    let theta_unwrapped = theta_old + sim.omega_rad_per_s * dt;
    let theta_new = wrap_cycle(theta_unwrapped);
    if theta_unwrapped >= CYCLE_RAD {
        sim.peak_pressure_pa_prev_cycle = sim.peak_pressure_pa_cycle;
        sim.peak_pressure_pa_cycle = 0.0;
        sim.peak_temperature_k_prev_cycle = sim.peak_temperature_k_cycle;
        sim.peak_temperature_k_cycle = 0.0;
    }

    // --- cylinder gas states and pressure-derived torque ---
    let mut torque_gas_nm = 0.0;
    let mut torque_pumping_nm = 0.0;
    let mut step_peak_pressure_pa: f64 = 0.0;
    let mut step_peak_temperature_k: f64 = 0.0;

    for index in 0..count {
        let mut c = sim.cylinders[index];
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
            // Intake valve closing: trap the charge and latch this cycle's fuel.
            let trapped_air_kg =
                air.intake_manifold_pressure_pa * volume_new_m3 * air.volumetric_efficiency
                    / (r * air.intake_manifold_temperature_k);
            c.mass_kg = trapped_air_kg;
            c.trapped_air_kg = trapped_air_kg;
            c.temperature_k = air.intake_manifold_temperature_k;
            c.pressure_pa = c.mass_kg * r * c.temperature_k / volume_new_m3;
            c.burned_fraction = 0.0;
            // Smoke limit: fuelling can never exceed the trapped air divided by
            // the minimum permitted air/fuel ratio.
            let smoke_limit_kg = trapped_air_kg / injection.smoke_limit_afr;
            c.fuel_charge_kg = (demand_mg * 1.0e-6).min(smoke_limit_kg).max(0.0);
            sim.last_fuel_charge_mg = c.fuel_charge_kg * 1.0e6;
        } else {
            match phase_new {
                Phase::Closed => {
                    let polytropic = if psi_new < 0.0 {
                        combustion.polytropic_compression
                    } else {
                        combustion.polytropic_expansion
                    };
                    let (heat_j, burned_fraction) = heat_release::heat_release_j(
                        &combustion,
                        c.fuel_charge_kg,
                        injection.fuel_lower_heating_value_j_per_kg,
                        c.burned_fraction,
                        psi_new,
                    );
                    c.burned_fraction = burned_fraction;

                    // dT = -(n - 1) T dV / V   +   dQ / (m cv)
                    let dv = volume_new_m3 - volume_old_m3;
                    let d_temperature_poly =
                        -(polytropic - 1.0) * c.temperature_k * (dv / volume_old_m3);
                    let d_temperature_burn = if c.mass_kg > 0.0 {
                        heat_j / (c.mass_kg * combustion.burned_gas_cv_j_per_kg_k)
                    } else {
                        0.0
                    };
                    c.temperature_k += d_temperature_poly + d_temperature_burn;
                    c.pressure_pa = c.mass_kg * r * c.temperature_k / volume_new_m3;
                }
                Phase::Intake => {
                    c.temperature_k = air.intake_manifold_temperature_k;
                    c.pressure_pa = air.intake_manifold_pressure_pa;
                    c.mass_kg = c.pressure_pa * volume_new_m3 / (r * c.temperature_k);
                    c.fuel_charge_kg = 0.0;
                    c.burned_fraction = 0.0;
                }
                Phase::Exhaust => {
                    c.pressure_pa = air.exhaust_manifold_pressure_pa;
                    c.mass_kg = c.pressure_pa * volume_new_m3 / (r * c.temperature_k);
                    c.fuel_charge_kg = 0.0;
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
        sim.cylinders[index] = c;
    }

    sim.peak_pressure_pa_cycle = sim.peak_pressure_pa_cycle.max(step_peak_pressure_pa);
    sim.peak_pressure_pa_session = sim.peak_pressure_pa_session.max(step_peak_pressure_pa);
    sim.peak_temperature_k_cycle = sim.peak_temperature_k_cycle.max(step_peak_temperature_k);

    // --- explicit resisting and driving torque terms ---
    let piston_speed_m_per_s = torque::piston_speed_m_per_s(&slider, sim.omega_rad_per_s);
    let peak_for_friction_pa = sim
        .peak_pressure_pa_cycle
        .max(sim.peak_pressure_pa_prev_cycle);
    let torque_friction_nm = torque::friction_torque_nm(
        torque::fmep_pa(&friction, peak_for_friction_pa, piston_speed_m_per_s),
        total_displacement_m3,
    );
    let torque_accessory_nm = torque::accessory_torque_nm(&load_cfg, sim.omega_rad_per_s);
    let torque_starter_nm = torque::starter_torque_nm(&load_cfg, sim.controls.starter, rpm);
    let torque_load_nm = sim.controls.load_torque_nm;

    let torque_net_nm = torque_gas_nm + torque_pumping_nm + torque_starter_nm
        - torque_friction_nm
        - torque_accessory_nm
        - torque_load_nm;

    // --- crank dynamics ---
    let mut omega = sim.omega_rad_per_s + (torque_net_nm / inertia_kg_m2) * dt;
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

    sim.omega_rad_per_s = omega;
    sim.crank_angle_rad = theta_new;
    sim.sim_time_s += dt;
    sim.steps_advanced += 1;
    sim.report = StepReport {
        torque_gas_nm,
        torque_pumping_nm,
        torque_friction_nm,
        torque_accessory_nm,
        torque_starter_nm,
        torque_load_nm,
        torque_net_nm,
    };

    Ok(())
}
