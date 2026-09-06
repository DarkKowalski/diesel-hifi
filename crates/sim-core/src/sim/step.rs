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
    acoustics, brake, driveline, egr, flow, gas, heat_release, heat_transfer, ignition_delay,
    injection, manifold, torque, turbo, wrap_cycle, SimState, StepReport,
};

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

    // --- air path: turbocharger, EGR, and manifold filling ---
    //
    // Boost is no longer an input. Both manifolds are control volumes whose
    // pressure is a state integrated from mass flow; the compressor and turbine
    // trade power through a shaft with inertia; and Milestone 2's boost schedule
    // has become the setpoint the wastegate controller regulates towards, which
    // is what the manual describes the MCM doing.
    //
    // Making pressure a state is also what removes the algebraic loop between
    // fuelling and available air that Milestone 2 had to break with a lag.
    let ambient_pa = air.ambient_pressure_pa;
    let events_per_s = count as f64 * rpm / 120.0;
    let fuel_flow_kg_per_s = demand_mg * 1.0e-6 * events_per_s;

    // --- engine brake ---
    //
    // Resolved before the air path, because stage III has the MCM actuating the
    // wastegate and the EGR positioner, and those commands replace the fuelled
    // calibration's control loops for as long as the brake is engaged.
    let brake_cmd = brake::command(
        &cfg.engine_brake,
        count,
        state.controls.brake_stage,
        state.controls.pedal,
        rpm,
    );
    state.brake = brake_cmd;

    let load_fraction = (demand_mg / inj.max_fuel_mg_per_cycle).clamp(0.0, 1.0);
    let boost_setpoint_pa =
        ambient_pa + (air.boost_target_schedule.lookup(rpm) - ambient_pa) * load_fraction;

    let turbo_out = turbo::evaluate(
        &cfg.turbo,
        air,
        gas_props,
        state.turbo_shaft_rad_per_s,
        state.intake.pressure_pa,
        state.exhaust.pressure_pa,
        state.exhaust.temperature_k,
        state.wastegate_position,
    );

    let egr_cooled_k = egr::cooler_outlet_temperature_k(
        &cfg.egr,
        state.exhaust.temperature_k,
        air.coolant_temperature_k,
    );
    let egr_flow_kg_per_s = egr::mass_flow_kg_per_s(
        &cfg.egr,
        gas_props,
        state.egr_valve_position,
        state.exhaust.pressure_pa,
        egr_cooled_k,
        state.intake.pressure_pa,
    );

    let engine_flow_kg_per_s = manifold::engine_flow_kg_per_s(
        derived.total_displacement_m3,
        air.volumetric_efficiency,
        rpm,
        state.intake.density_kg_per_m3(gas_props),
    );

    // --- actuators ---
    state.egr_rate = egr::rate(egr_flow_kg_per_s, turbo_out.compressor_flow_kg_per_s);
    if brake_cmd.active {
        // While braking the MCM drives the EGR positioner directly rather than
        // closing a loop on recirculation rate. It has to: the rate schedule is a
        // combustion calibration, and there is no combustion here. What the valve
        // is for now is filling the cylinder, which is a position, not a ratio.
        state.egr_valve_position = brake_cmd.egr_command;
        state.egr_integral = 0.0;
    } else {
        let egr_target = cfg.egr.rate_schedule.lookup(rpm).min(cfg.egr.max_rate);
        state.egr_valve_position = egr::update_valve(
            &cfg.egr,
            state.egr_valve_position,
            &mut state.egr_integral,
            state.egr_rate,
            egr_target,
            state.controls.egr_enabled,
            dt,
        );
    }
    // While braking the MCM regulates to its own boost setpoint, through a
    // deliberately slower loop. The brake changes the plant the wastegate is
    // driving by about an order of magnitude, and the gains calibrated for the
    // fuelled engine hunt against it; see `engine_brake.wastegate_gain_scale`.
    let (wastegate_setpoint_pa, wastegate_gains) = if brake_cmd.active {
        (
            brake_cmd.boost_target_pa,
            turbo::scaled_wastegate_gains(&cfg.turbo, brake_cmd.wastegate_gain_scale),
        )
    } else {
        (
            boost_setpoint_pa,
            turbo::scaled_wastegate_gains(&cfg.turbo, 1.0),
        )
    };
    state.wastegate_position = turbo::update_wastegate(
        &wastegate_gains,
        state.wastegate_position,
        &mut state.wastegate_integral,
        state.intake.pressure_pa,
        wastegate_setpoint_pa,
        dt,
    );
    state.turbo_shaft_rad_per_s = turbo::advance_shaft(
        &cfg.turbo,
        state.turbo_shaft_rad_per_s,
        turbo_out.shaft_power_balance_w,
        dt,
    );

    // --- intake manifold: compressor and EGR in, engine out ---
    let intake_mass_kg = state
        .intake
        .mass_kg(gas_props, air.intake_manifold_volume_m3);
    state.intake.temperature_k = manifold::mix_temperature(
        state.intake.temperature_k,
        turbo_out.charge_temperature_k,
        turbo_out.compressor_flow_kg_per_s,
        intake_mass_kg,
        dt,
    );
    state.intake.temperature_k = manifold::mix_temperature(
        state.intake.temperature_k,
        egr_cooled_k,
        egr_flow_kg_per_s,
        intake_mass_kg,
        dt,
    );
    state.intake.burned_fraction = manifold::mix_burned_fraction(
        state.intake.burned_fraction,
        turbo_out.compressor_flow_kg_per_s,
        egr_flow_kg_per_s * state.exhaust.burned_fraction,
        intake_mass_kg,
        dt,
    );
    state.intake.pressure_pa = manifold::advance_pressure(
        gas_props,
        state.intake.pressure_pa,
        state.intake.temperature_k,
        air.intake_manifold_volume_m3,
        turbo_out.compressor_flow_kg_per_s + egr_flow_kg_per_s - engine_flow_kg_per_s,
        dt,
        ambient_pa * 0.5,
    );

    // --- exhaust manifold: cylinders in, turbine, wastegate and EGR out ---
    state.exhaust.pressure_pa = manifold::advance_pressure(
        gas_props,
        state.exhaust.pressure_pa,
        state.exhaust.temperature_k,
        air.exhaust_manifold_volume_m3,
        engine_flow_kg_per_s + fuel_flow_kg_per_s
            - turbo_out.turbine_flow_kg_per_s
            - turbo_out.wastegate_flow_kg_per_s
            - egr_flow_kg_per_s,
        dt,
        ambient_pa * 0.5,
    );

    state.compressor_flow_kg_per_s = turbo_out.compressor_flow_kg_per_s;
    state.turbine_flow_kg_per_s = turbo_out.turbine_flow_kg_per_s;
    state.wastegate_flow_kg_per_s = turbo_out.wastegate_flow_kg_per_s;
    state.egr_flow_kg_per_s = egr_flow_kg_per_s;
    state.engine_flow_kg_per_s = engine_flow_kg_per_s;

    let manifold_pa = state.intake.pressure_pa;
    let manifold_k = state.intake.temperature_k;
    let manifold_burned = state.intake.burned_fraction;
    let exhaust_pa = state.exhaust.pressure_pa;
    let exhaust_k = state.exhaust.temperature_k;

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

    // Port geometry common to every cylinder. The effective area is then
    // trimmed per cylinder below, because no two ports in a real head flow
    // identically and six identical ports are audible as such.
    let nominal_exhaust_port = acoustics::Port {
        effective_area_m2: valvetrain.exhaust_effective_area_m2,
        valve_open_rad: valvetrain.exhaust_valve_open_rad,
        ramp_rad: valvetrain.exhaust_ramp_rad,
    };

    // Acoustic source, summed across whichever cylinders are blowing down, and
    // the port-area-weighted temperature of the gas they are handing to the
    // exhaust manifold.
    let mut acoustic_source = 0.0;
    let mut donor_temperature_sum = 0.0;
    let mut donor_weight = 0.0;

    // Summed cylinder pressure, which drives the structural radiation path.
    // Every cylinder counts here, open or shut: the block is shaken by the
    // pressure inside it whether or not a valve happens to be off its seat.
    // That is exactly what distinguishes this path from the exhaust one.
    let mut cylinder_pressure_sum_pa = 0.0;

    for index in 0..count {
        let mut c = state.cylinders[index];
        let psi_old = cylinder::signed_cycle_angle(theta_old, c.phase_offset_rad);
        let psi_new = cylinder::signed_cycle_angle(theta_new, c.phase_offset_rad);

        let exhaust_port = acoustics::Port {
            effective_area_m2: nominal_exhaust_port.effective_area_m2 * c.exhaust_area_trim,
            ..nominal_exhaust_port
        };
        let phase_new = cylinder::phase_at(
            psi_new,
            valvetrain.intake_valve_close_rad,
            valvetrain.exhaust_valve_open_rad,
        );
        let volume_old_m3 = slider.volume_m3(psi_old);
        let volume_new_m3 = slider.volume_m3(psi_new);

        // Brake cam opening for this cylinder, zero unless the brake is engaged
        // and this cylinder is one of the ones the stage acts on.
        let brake_area_m2 = if brake_cmd.brakes_cylinder(index) {
            brake::port_area_m2(&cfg.engine_brake, psi_new)
        } else {
            0.0
        };

        if phase_new == Phase::Closed && c.phase != Phase::Closed {
            // --- intake valve closing ---
            //
            // Trap the charge drawn from the intake manifold on top of whatever
            // burned gas survived the exhaust stroke, then schedule this cycle's
            // injection.
            //
            // Two things now dilute the oxygen the smoke limit sees: residual
            // gas left in the clearance volume, and recirculated exhaust already
            // mixed into the intake manifold. Only the genuinely fresh part of
            // the charge counts as air.
            let ideal_charge_kg = gas::mass_kg(gas_props, manifold_pa, manifold_k, volume_new_m3)
                * air.volumetric_efficiency;
            let residual_kg = c.residual_kg.max(0.0);
            let fresh_kg = (ideal_charge_kg - residual_kg).max(0.0);
            let air_kg = fresh_kg * (1.0 - manifold_burned).clamp(0.0, 1.0);
            let total_kg = residual_kg + fresh_kg;
            let mixed_k = if total_kg > 0.0 {
                (residual_kg * c.residual_temperature_k + fresh_kg * manifold_k) / total_kg
            } else {
                manifold_k
            };

            c.mass_kg = total_kg;
            c.trapped_air_kg = air_kg;
            c.temperature_k = mixed_k;
            c.pressure_pa = gas::pressure_pa(gas_props, total_kg, mixed_k, volume_new_m3);
            c.motored_pressure_pa = c.pressure_pa;
            c.motored_temperature_k = mixed_k;
            c.burned_fraction = 0.0;
            c.wall_heat_loss_j = 0.0;
            c.profile_ready = false;
            c.profile = heat_release::Profile::NONE;

            let smoke_limit_kg = air_kg / inj.smoke_limit_afr;
            // The trim scales what this injector delivers for a given command.
            // The smoke limit still applies after it: a generous injector on a
            // cylinder short of air is still held to the air it has.
            let commanded_kg = demand_mg * 1.0e-6 * c.fuel_trim;
            let fuel_kg = commanded_kg.min(smoke_limit_kg).max(0.0);
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

                    // --- decompression brake ---
                    //
                    // The brake cam cracks an exhaust valve twice while the
                    // cylinder is otherwise shut: once early in compression, to
                    // let boosted manifold gas *in* and make the coming
                    // compression more expensive, and once just before firing
                    // TDC, to throw that compression away instead of returning
                    // it to the piston on expansion.
                    //
                    // `dv` is passed as zero because the piston work for this
                    // step has already been taken in the temperature update
                    // above. This transfer is mass and enthalpy only.
                    if brake_area_m2 > 0.0 {
                        let after = flow::exchange(
                            gas_props,
                            flow::Charge {
                                mass_kg: c.mass_kg,
                                temperature_k: c.temperature_k,
                                pressure_pa: c.pressure_pa,
                            },
                            &flow::PortState {
                                area_m2: brake_area_m2,
                                pressure_pa: exhaust_pa,
                                temperature_k: exhaust_k,
                            },
                            volume_new_m3,
                            0.0,
                            dt,
                        );
                        c.mass_kg = after.mass_kg;
                        c.temperature_k = after.temperature_k;
                        c.pressure_pa = after.pressure_pa;
                        c.motored_pressure_pa = c.pressure_pa;
                    }
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
                    // Blowdown through the exhaust port, as real orifice flow.
                    //
                    // Milestone 2 clamped the cylinder to the manifold here.
                    // That is adequate for pumping work, but it makes the
                    // pressure difference across the port identically zero — and
                    // that difference *is* the exhaust pulse this milestone has
                    // to produce. A boundary condition cannot make a sound.
                    //
                    // Only the exhaust side gets orifice flow. The intake stays
                    // a manifold boundary because it sits near equilibrium,
                    // whereas the exhaust valve opens onto a pressure ratio
                    // large enough to choke.
                    let after = flow::exchange(
                        gas_props,
                        flow::Charge {
                            mass_kg: c.mass_kg,
                            temperature_k: c.temperature_k,
                            pressure_pa: c.pressure_pa,
                        },
                        &flow::PortState {
                            area_m2: exhaust_port.area_m2(psi_new),
                            pressure_pa: exhaust_pa,
                            temperature_k: exhaust_k,
                        },
                        volume_new_m3,
                        volume_new_m3 - volume_old_m3,
                        dt,
                    );

                    c.mass_kg = after.mass_kg;
                    c.temperature_k = after.temperature_k;
                    c.pressure_pa = after.pressure_pa;
                    c.motored_pressure_pa = c.pressure_pa;
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

        // The exhaust pulse is a flow through a port, so it exists only while
        // that port is open and scales with how far open it is.
        acoustic_source += acoustics::cylinder_source(
            phase_new,
            psi_new,
            c.pressure_pa,
            exhaust_pa,
            ambient_pa,
            &exhaust_port,
            brake_area_m2,
        );
        cylinder_pressure_sum_pa += c.pressure_pa;

        if phase_new == Phase::Exhaust {
            let weight = exhaust_port.area_m2(psi_new);
            donor_temperature_sum += weight * c.temperature_k;
            donor_weight += weight;
        }

        step_peak_pressure_pa = step_peak_pressure_pa.max(c.pressure_pa);
        step_peak_temperature_k = step_peak_temperature_k.max(c.temperature_k);
        state.cylinders[index] = c;
    }

    // --- exhaust manifold thermal state, from the cylinders that just emptied ---
    if donor_weight > 0.0 {
        let donor_k = donor_temperature_sum / donor_weight;
        let exhaust_mass_kg = state
            .exhaust
            .mass_kg(gas_props, air.exhaust_manifold_volume_m3);
        state.exhaust.temperature_k = manifold::mix_temperature(
            state.exhaust.temperature_k,
            donor_k,
            engine_flow_kg_per_s + fuel_flow_kg_per_s,
            exhaust_mass_kg,
            dt,
        );
    }

    // Burned-gas fraction of the exhaust: the fuel and the air it consumed,
    // against everything leaving the cylinders. A diesel runs lean, so this
    // stays well below one and the recirculated gas still carries oxygen.
    let exhaust_total_flow = engine_flow_kg_per_s + fuel_flow_kg_per_s;
    if exhaust_total_flow > 0.0 {
        state.exhaust.burned_fraction = (fuel_flow_kg_per_s * (1.0 + inj.stoichiometric_afr)
            / exhaust_total_flow)
            .clamp(0.0, 1.0);
    }

    // --- acoustic sample, one per step, both radiating paths ---
    //
    // Normalising by ambient makes the structural forcing dimensionless, which
    // is the same thing `cylinder_source` does for the exhaust term, and keeps
    // the calibrated gain from carrying a unit conversion inside it.
    let structural_forcing = if ambient_pa > 0.0 {
        cylinder_pressure_sum_pa / ambient_pa
    } else {
        0.0
    };
    state
        .acoustics
        .push(&cfg.audio, acoustic_source, structural_forcing, dt);

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

    // --- driveline ---
    //
    // The abstract load torque above and the road load here are deliberately
    // separate terms. One is a dynamometer knob; the other is a truck on a road.
    // Summing them into a single number would make the telemetry unable to say
    // which of the two is holding the engine back.
    let driveline_out = driveline::evaluate(
        &cfg.driveline,
        air,
        gas_props,
        state.controls.gear,
        state.controls.road_grade_percent,
        state.omega_rad_per_s,
    );
    state.driveline = driveline_out;
    let torque_driveline_nm = driveline_out.road_torque_nm;

    let torque_net_nm = torque_gas_nm + torque_pumping_nm + torque_starter_nm
        - torque_friction_nm
        - torque_accessory_nm
        - torque_load_nm
        - torque_driveline_nm;

    state.report = StepReport {
        torque_gas_nm,
        torque_pumping_nm,
        torque_friction_nm,
        torque_accessory_nm,
        torque_starter_nm,
        torque_load_nm,
        torque_driveline_nm,
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
    //
    // In gear, the truck is rigidly geared to the crankshaft, so its mass appears
    // here as inertia. Forty tonnes through a high gear reflects two orders of
    // magnitude more than the engine's own rotating inertia, which is exactly why
    // a laden truck on a long descent needs a brake that does not wear out.
    let inertia_kg_m2 = cfg.inertia.rotating_inertia_kg_m2 + driveline_out.reflected_inertia_kg_m2;
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
