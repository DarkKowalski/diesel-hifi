#!/usr/bin/env python3
"""Emit the schema-3 OM 471.9 M3D configuration document.

Kept in the repository because the provenance table is the authoritative record
of which values came from the manual and which are our calibration. Regenerate
with:

    python3 scripts/gen-om471-config.py

Nothing reads this at build or run time; the JSON it writes is what ships.
"""

import json
import math
from pathlib import Path

OUT = Path("crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json")

SRC = "mb-om471-intro-2011"
L_TECH = (
    "SN00.00-W-0002-05H, Technical data of diesel engine OM 471, "
    "printed page 11 (PDF page 14)"
)
L_ENGINE = "SN00.00-W-0002-04H, Engine OM 471, printed page 10 (PDF page 13)"
L_FUEL = (
    "GF47.00-W-3013H, Fuel high pressure circuit function "
    "(471.9 in MODEL 963, code M5Z), printed page 100 (PDF page 103)"
)

L_TURBO = (
    "GF09.40-W-0001H, Boost pressure control and turbocharger protection "
    "function, printed pages 35-36 (PDF pages 38-39)"
)
L_EGR = (
    "GF14.20-W-3000H, Exhaust gas recirculation function "
    "(ENGINES 471.9 in MODEL 963, 964), printed page 66 (PDF page 69)"
)

DEG = math.pi / 180.0


def rad(deg):
    return round(deg * DEG, 7)


# The manual publishes the EGR cooler duty as a temperature pair: exhaust enters
# at about 650 C and leaves at about 170 C. Effectiveness is derived from those
# two figures against coolant temperature rather than invented separately.
COOLANT_K = 358.15
EGR_COOLER_IN_K = 923.15
EGR_COOLER_OUT_K = 443.15
EGR_EFFECTIVENESS = round(
    (EGR_COOLER_IN_K - EGR_COOLER_OUT_K) / (EGR_COOLER_IN_K - COOLANT_K), 7
)

config = {
    "schema_version": 3,
    "identity": {
        "id": "mercedes-benz-om471-9-m3d-375kw",
        "display_name": "OM 471.9 M3D 375 kW Reference",
        "manufacturer_reference": (
            "Mercedes-Benz OM 471.9, power code M3D "
            "(engine series 471.9 in model 963/964)"
        ),
        "power_code": "M3D",
        "disclaimer": (
            "Public-spec reference simulation built from published service-literature "
            "figures. Not an OEM calibration, not a digital twin, and not endorsed by "
            "Mercedes-Benz. Manufacturer and engine names are used as factual references "
            "only. Values marked calibrated are our estimates, not OEM data. The engine "
            "speeds at which this model reaches the published 375 kW and 2500 Nm are an "
            "outcome of our calibration; the manual does not publish them."
        ),
    },
    "geometry": {
        "cylinders": 6,
        "bore_m": 0.132,
        "stroke_m": 0.156,
        "connecting_rod_m": 0.268,
        "compression_ratio": 17.3,
        "firing_order": [1, 5, 3, 6, 2, 4],
        "published_displacement_m3": 0.0128,
        "published_stroke_bore_ratio": 1.18,
    },
    "valvetrain": {
        "intake_valves_per_cylinder": 2,
        "exhaust_valves_per_cylinder": 2,
        "intake_valve_close_rad": rad(-160.0),
        "exhaust_valve_open_rad": rad(140.0),
        "exhaust_ramp_rad": rad(35.0),
        "exhaust_effective_area_m2": 0.0025,
    },
    "injection": {
        "rail_pressure_max_pa": 90000000.0,
        "amplified_pressure_max_pa": 210000000.0,
        "fuel_lower_heating_value_j_per_kg": 42700000.0,
        "fuel_density_kg_m3": 832.0,
        "max_fuel_mg_per_cycle": 350.0,
        "smoke_limit_afr": 19.5,
        "stoichiometric_afr": 14.5,
        "nozzle_hole_count": 8,
        "nozzle_hole_diameter_m": 0.000175,
        "discharge_coefficient": 0.75,
        "amplified_variant_min_fuel_mg": 120.0,
        "soi_schedule": {
            "breakpoints": [600.0, 1000.0, 1400.0, 1800.0, 2100.0],
            "values": [rad(-8.0), rad(-10.0), rad(-12.0), rad(-14.0), rad(-15.0)],
        },
        "soi_load_retard_rad_per_mg": 0.00075,
    },
    "combustion": {
        "combustion_efficiency": 0.98,
        "cetane_number": 50.0,
        "premixed_wiebe_shape": 2.0,
        "premixed_wiebe_efficiency": 6.908,
        "premixed_duration_rad": rad(10.0),
        "diffusion_wiebe_shape": 0.9,
        "diffusion_wiebe_efficiency": 6.908,
        "diffusion_duration_rad_per_mg": 0.0028,
        "diffusion_duration_min_rad": rad(15.0),
        "max_premixed_fraction": 0.85,
    },
    "heat_transfer": {
        "enabled": True,
        "woschni_c1_closed": 2.28,
        "woschni_c1_gas_exchange": 6.18,
        "woschni_c2": 0.00324,
        "wall_temperature_k": 480.0,
    },
    "gas": {
        "gas_constant_j_per_kg_k": 287.0528,
        "cv_reference_j_per_kg_k": 718.0,
        "cv_slope_j_per_kg_k2": 0.18,
        "reference_temperature_k": 300.0,
    },
    "friction": {
        "fmep_constant_pa": 60000.0,
        "fmep_peak_pressure_coeff": 0.005,
        "fmep_piston_speed_coeff_pa_s_per_m": 1200.0,
        "fmep_piston_speed_sq_coeff": 130.0,
    },
    "air_path": {
        "ambient_pressure_pa": 101325.0,
        "ambient_temperature_k": 293.15,
        "boost_target_schedule": {
            "breakpoints": [
                600.0,
                800.0,
                1000.0,
                1100.0,
                1300.0,
                1500.0,
                1600.0,
                1700.0,
                1800.0,
                1900.0,
                2100.0,
            ],
            "values": [
                128000.0,
                188000.0,
                286000.0,
                308000.0,
                299000.0,
                278000.0,
                256000.0,
                249000.0,
                235000.0,
                224000.0,
                195000.0,
            ],
        },
        "intake_manifold_volume_m3": 0.020,
        "exhaust_manifold_volume_m3": 0.010,
        "intercooler_effectiveness": 0.80,
        "coolant_temperature_k": 358.15,
        "exhaust_restriction_pa_per_kg2_s2": 120000.0,
        "crankcase_pressure_pa": 101325.0,
        "volumetric_efficiency": 0.92,
    },
    "turbo": {
        "shaft_inertia_kg_m2": 3.5e-05,
        "compressor_wheel_diameter_m": 0.10,
        "compressor_efficiency": 0.72,
        "compressor_head_coefficient": 0.70,
        "compressor_max_flow_kg_s_per_rad_s": 6.5e-05,
        "turbine_effective_area_m2": 0.0006,
        "turbine_efficiency": 0.70,
        "bearing_friction_nm_per_rad_s": 1.0e-08,
        "max_shaft_speed_rad_per_s": 15000.0,
        "wastegate_max_area_m2": 0.0025,
        "wastegate_p_gain_per_pa": 1.5e-05,
        "wastegate_i_gain_per_pa_s": 3.0e-05,
        "wastegate_slew_per_s": 6.0,
    },
    "egr": {
        "cooler_inlet_temperature_k": EGR_COOLER_IN_K,
        "cooler_outlet_temperature_k": EGR_COOLER_OUT_K,
        "cooler_effectiveness": EGR_EFFECTIVENESS,
        "rate_schedule": {
            "breakpoints": [600.0, 900.0, 1100.0, 1400.0, 1600.0, 1900.0, 2100.0],
            "values": [0.12, 0.14, 0.12, 0.10, 0.08, 0.06, 0.04],
        },
        "valve_max_area_m2": 0.0004,
        "valve_discharge_coefficient": 0.70,
        "rate_p_gain": 20.0,
        "rate_i_gain_per_s": 60.0,
        "max_rate": 0.35,
    },
    "governor": {
        "idle_target_rpm": 560.0,
        "idle_p_gain_mg_per_rad_s": 1.2,
        "idle_i_gain_mg_per_rad": 0.6,
        "idle_integral_limit_mg": 40.0,
        "overspeed_taper_start_rpm": 1900.0,
        "overspeed_cutoff_rpm": 2100.0,
    },
    "load": {
        "accessory_torque_constant_nm": 25.0,
        "accessory_torque_per_rad_s": 0.02,
        "max_external_load_nm": 5000.0,
        "starter_torque_nm": 1500.0,
        "starter_cutout_rpm": 300.0,
    },
    "inertia": {
        "rotating_inertia_kg_m2": 3.5,
        "complete_engine_mass_kg": 1200.0,
    },
    "limits": {
        "max_combustion_pressure_pa": 23000000.0,
        "max_rpm": 2400.0,
        "max_gas_temperature_k": 3000.0,
    },
    "solver": {
        "fixed_step_s": 0.000025,
        "max_steps_per_batch": 20000,
    },
    "audio": {
        "reference_spl_db": 94.0,
        "exhaust_gain": 12000.0,
        "highpass_cutoff_hz": 20.0,
        "lowpass_cutoff_hz": 6000.0,
        "soft_clip_knee": 0.85,
    },
    "rated": {
        "max_power_w": 375000.0,
        "max_power_hp": 510.0,
        "max_torque_nm": 2500.0,
    },
    "sources": [
        {
            "id": SRC,
            "title": "Introduction of engine OM 471 and exhaust aftertreatment",
            "publisher": "Daimler AG, Mercedes-Benz service literature",
            "technical_status": "2011-09-01",
            "order_number": "6517 1260 02",
            "scope": (
                "Engine series 471.9 in model 963/964. M3D power code, and documented "
                "M5Z Euro VI subsystems where explicitly stated. Later 390 kW / 2600 Nm "
                "/ 2700 bar OM 471 figures are NOT part of this configuration."
            ),
            "local_path": "datasheet/om-471-en.pdf",
        }
    ],
}


def pub(path, note, locator):
    return {
        "path": path,
        "status": "published",
        "value_note": note,
        "source_id": SRC,
        "locator": locator,
    }


def der(path, note, formula, inputs):
    return {
        "path": path,
        "status": "derived",
        "value_note": note,
        "formula": formula,
        "inputs": inputs,
    }


def cal(path, note, purpose, lo, hi):
    return {
        "path": path,
        "status": "calibrated",
        "value_note": note,
        "purpose": purpose,
        "safe_range": [lo, hi],
    }


NOT_PUBLISHED = "The source manual does not publish this."

provenance = [
    # --- geometry -----------------------------------------------------------
    pub("geometry.cylinders", "6 (in line)", L_TECH),
    pub(
        "geometry.bore_m",
        "132 mm (piston diameter and cylinder diameter both listed as 132 mm)",
        L_TECH,
    ),
    pub("geometry.stroke_m", "156 mm", L_TECH),
    pub("geometry.connecting_rod_m", "268 mm", L_TECH + ", Connecting rod"),
    pub("geometry.compression_ratio", "17,3", L_TECH),
    cal(
        "geometry.firing_order",
        "1-5-3-6-2-4, the conventional inline-six sequence. " + NOT_PUBLISHED,
        "Even 120-degree firing spacing, so cylinder phase offsets are data-driven "
        "rather than hard-coded in the solver.",
        1.0,
        6.0,
    ),
    pub(
        "geometry.published_displacement_m3",
        "12,8 l; retained only to cross-check the displacement recomputed from bore, "
        "stroke and cylinder count",
        L_TECH,
    ),
    pub(
        "geometry.published_stroke_bore_ratio",
        "1,18; cross-checked against stroke / bore",
        L_TECH,
    ),
    # --- valvetrain ---------------------------------------------------------
    pub(
        "valvetrain.intake_valves_per_cylinder",
        "2 intake valves per cylinder; valve control DOHC, valve number 2/2",
        L_TECH,
    ),
    pub(
        "valvetrain.exhaust_valves_per_cylinder",
        "2 exhaust valves per cylinder; valve control DOHC, valve number 2/2",
        L_TECH,
    ),
    cal(
        "valvetrain.intake_valve_close_rad",
        "160 degrees before firing TDC, i.e. 20 degrees after intake BDC. "
        + NOT_PUBLISHED,
        "Boundary between the intake-manifold boundary condition and the closed "
        "compression period.",
        -3.05,
        -2.1,
    ),
    cal(
        "valvetrain.exhaust_valve_open_rad",
        "140 degrees after firing TDC. " + NOT_PUBLISHED,
        "Boundary between the closed expansion period and the exhaust-manifold "
        "boundary condition, giving blowdown before BDC.",
        2.0,
        3.05,
    ),
    cal(
        "valvetrain.exhaust_ramp_rad",
        "35 degrees of crank between shut and full lift. The manual publishes no "
        "valve lift profile.",
        "Sets how sharply blowdown starts. A slow ramp leaves the port nearly shut "
        "at peak pressure difference, which makes the exhaust pulse a smooth hump "
        "with no harmonic content and no audible sound.",
        0.05,
        1.5,
    ),
    cal(
        "valvetrain.exhaust_effective_area_m2",
        "2.5e-3 m^2 for both exhaust valves at full lift. The manual publishes the "
        "valve count but no lift profile or flow area.",
        "Scales the exhaust blowdown that drives the acoustic source: a bigger port "
        "makes a louder pulse.",
        0.0001,
        0.02,
    ),
    # --- injection ----------------------------------------------------------
    pub(
        "injection.rail_pressure_max_pa",
        "900 bar maximum rail pressure; drives nozzle flow for the standard "
        "(non-amplified) APCRS variant",
        L_TECH + "; also " + L_ENGINE,
    ),
    pub(
        "injection.amplified_pressure_max_pa",
        "APCRS injection pressure up to 2100 bar after local pressure amplification; "
        "drives nozzle flow for the amplified variant",
        L_FUEL,
    ),
    cal(
        "injection.fuel_lower_heating_value_j_per_kg",
        "42.7 MJ/kg, a general diesel-fuel reference figure. Not from the manual.",
        "Converts injected fuel mass into released heat.",
        41.0e6,
        44.0e6,
    ),
    cal(
        "injection.fuel_density_kg_m3",
        "832 kg/m3, a general diesel-fuel reference figure. Not from the manual.",
        "Sets nozzle mass flow through the Bernoulli discharge relation.",
        800.0,
        860.0,
    ),
    cal(
        "injection.max_fuel_mg_per_cycle",
        "350 mg per cylinder per cycle at full pedal. " + NOT_PUBLISHED,
        "Upper bound on pedal fuel demand, before the air-limited smoke clip.",
        100.0,
        600.0,
    ),
    cal(
        "injection.smoke_limit_afr",
        "19.5:1 minimum air/fuel ratio. " + NOT_PUBLISHED,
        "Clips fuelling to the trapped fresh air, so the boost schedule rather than "
        "the pedal sets the full-load torque curve.",
        16.0,
        30.0,
    ),
    cal(
        "injection.stoichiometric_afr",
        "14.5:1, the standard figure for diesel fuel. " + NOT_PUBLISHED,
        "Converts burned fuel into mass of combustion products, which is what "
        "recirculated exhaust actually displaces. Without it, EGR would be treated "
        "as inert and its penalty overstated.",
        13.0,
        16.0,
    ),
    cal(
        "injection.nozzle_hole_count",
        "8 holes. " + NOT_PUBLISHED,
        "Total nozzle discharge area, which sets injection duration.",
        4.0,
        12.0,
    ),
    cal(
        "injection.nozzle_hole_diameter_m",
        "175 micrometres. " + NOT_PUBLISHED,
        "Total nozzle discharge area, which sets injection duration.",
        0.0001,
        0.00030,
    ),
    cal(
        "injection.discharge_coefficient",
        "0.75. " + NOT_PUBLISHED,
        "Nozzle efficiency in the Bernoulli discharge relation.",
        0.5,
        0.95,
    ),
    cal(
        "injection.amplified_variant_min_fuel_mg",
        "120 mg. The manual states the MCM selects the injection variant by operating "
        "condition but does not publish the threshold.",
        "Selects between the published amplified and non-amplified APCRS variants.",
        0.0,
        350.0,
    ),
    cal(
        "injection.soi_schedule",
        "8 to 15 degrees before firing TDC, advancing with engine speed. "
        + NOT_PUBLISHED,
        "Base injection timing against speed. Combustion phasing sets both efficiency "
        "and peak cylinder pressure.",
        -1.0,
        1.0,
    ),
    cal(
        "injection.soi_load_retard_rad_per_mg",
        "0.00075 rad per mg, about 8 degrees of retard at full fuelling. "
        + NOT_PUBLISHED,
        "Retards injection as fuelling rises, which is what keeps peak cylinder "
        "pressure inside the published 230 bar envelope at high load.",
        0.0,
        0.002,
    ),
    # --- combustion ---------------------------------------------------------
    cal(
        "combustion.combustion_efficiency",
        "0.98 of the fuel lower heating value released. " + NOT_PUBLISHED,
        "Accounts for incomplete combustion in the single-zone model.",
        0.85,
        1.0,
    ),
    cal(
        "combustion.cetane_number",
        "50, a typical European automotive diesel value. Not from the manual.",
        "Sets the Hardenberg-Hase activation energy and therefore ignition delay.",
        40.0,
        60.0,
    ),
    cal(
        "combustion.premixed_wiebe_shape",
        "Wiebe shape exponent m = 2.0 for the premixed phase. " + NOT_PUBLISHED,
        "Sharpness of the premixed heat-release spike.",
        0.5,
        6.0,
    ),
    cal(
        "combustion.premixed_wiebe_efficiency",
        "Wiebe efficiency a = 6.908, the standard value for 99.9 per cent burn.",
        "Completeness of the premixed Wiebe term.",
        1.0,
        12.0,
    ),
    cal(
        "combustion.premixed_duration_rad",
        "10 degrees. " + NOT_PUBLISHED,
        "Duration of the premixed heat-release phase.",
        0.05,
        0.6,
    ),
    cal(
        "combustion.diffusion_wiebe_shape",
        "Wiebe shape exponent m = 0.9 for the diffusion phase. " + NOT_PUBLISHED,
        "Shape of the mixing-controlled burn, slower than the premixed spike.",
        0.3,
        3.0,
    ),
    cal(
        "combustion.diffusion_wiebe_efficiency",
        "Wiebe efficiency a = 6.908, the standard value for 99.9 per cent burn.",
        "Completeness of the diffusion Wiebe term.",
        1.0,
        12.0,
    ),
    cal(
        "combustion.diffusion_duration_rad_per_mg",
        "0.0028 rad per mg, about 48 degrees at full fuelling. " + NOT_PUBLISHED,
        "Diffusion burn lengthens with injected quantity, which is what limits peak "
        "pressure and sets expansion-stroke efficiency at high load.",
        0.0005,
        0.01,
    ),
    cal(
        "combustion.diffusion_duration_min_rad",
        "15 degrees floor. " + NOT_PUBLISHED,
        "Stops the diffusion burn collapsing to an instantaneous release at very light "
        "fuelling.",
        0.05,
        1.0,
    ),
    cal(
        "combustion.max_premixed_fraction",
        "0.85. " + NOT_PUBLISHED,
        "Caps the premixed fraction so a long light-load ignition delay cannot produce "
        "a fully premixed burn.",
        0.2,
        1.0,
    ),
    # --- heat transfer ------------------------------------------------------
    cal(
        "heat_transfer.enabled",
        "true. Disabling exists only so tests can prove the term is wired in.",
        "Test affordance, not an operating mode.",
        0.0,
        1.0,
    ),
    cal(
        "heat_transfer.woschni_c1_closed",
        "2.28, the standard Woschni coefficient for the closed period.",
        "Gas velocity from piston motion during compression and expansion.",
        1.0,
        4.0,
    ),
    cal(
        "heat_transfer.woschni_c1_gas_exchange",
        "6.18, the standard Woschni coefficient for gas exchange.",
        "Gas velocity from piston motion during intake and exhaust.",
        3.0,
        9.0,
    ),
    cal(
        "heat_transfer.woschni_c2",
        "3.24e-3, the standard Woschni combustion-velocity coefficient.",
        "Additional gas velocity from the pressure rise combustion causes.",
        0.0,
        0.01,
    ),
    cal(
        "heat_transfer.wall_temperature_k",
        "480 K lumped wall temperature. " + NOT_PUBLISHED,
        "Single wall temperature for head, crown and liner, driving the heat-loss "
        "term that replaces Milestone 1's lumped polytropic exponents.",
        350.0,
        650.0,
    ),
    # --- gas ----------------------------------------------------------------
    der(
        "gas.gas_constant_j_per_kg_k",
        "287.0528 J/(kg K), the specific gas constant of dry air",
        "R = R_universal / M_air",
        [
            "R_universal = 8.314462618 J/(mol K)",
            "M_air = 0.0289647 kg/mol (dry air)",
        ],
    ),
    cal(
        "gas.cv_reference_j_per_kg_k",
        "718 J/(kg K), air near 300 K. General reference value, not from the manual.",
        "Base of the linear specific-heat model.",
        600.0,
        900.0,
    ),
    cal(
        "gas.cv_slope_j_per_kg_k2",
        "0.18 J/(kg K^2), reaching about 1024 J/(kg K) at 2000 K.",
        "Cheap stand-in for variable specific heats across the combustion "
        "temperature range.",
        0.0,
        0.5,
    ),
    cal(
        "gas.reference_temperature_k",
        "300 K reference for the linear specific-heat fit.",
        "Anchor temperature of the specific-heat model.",
        250.0,
        400.0,
    ),
    # --- friction -----------------------------------------------------------
    cal(
        "friction.fmep_constant_pa",
        "0.60 bar constant term. The manual publishes no friction map.",
        "Speed-independent friction in the Chen-Flynn style FMEP model.",
        20000.0,
        150000.0,
    ),
    cal(
        "friction.fmep_peak_pressure_coeff",
        "0.005 of peak cylinder pressure. The manual publishes no friction map.",
        "Load-dependent ring and bearing friction.",
        0.0,
        0.02,
    ),
    cal(
        "friction.fmep_piston_speed_coeff_pa_s_per_m",
        "1200 Pa per (m/s) of mean piston speed. The manual publishes no friction map.",
        "Hydrodynamic friction proportional to piston speed.",
        0.0,
        5000.0,
    ),
    cal(
        "friction.fmep_piston_speed_sq_coeff",
        "130 Pa per (m/s)^2. The manual publishes no friction map.",
        "Windage and turbulent losses rising with the square of piston speed.",
        0.0,
        600.0,
    ),
    # --- air path -----------------------------------------------------------
    cal(
        "air_path.ambient_pressure_pa",
        "101325 Pa, standard sea-level atmosphere.",
        "Reference pressure for boost, and the manifold pressure at zero fuelling.",
        80000.0,
        105000.0,
    ),
    cal(
        "air_path.ambient_temperature_k",
        "293.15 K ambient air.",
        "Reference intake air temperature upstream of the compressor.",
        250.0,
        320.0,
    ),
    cal(
        "air_path.boost_target_schedule",
        "1.28 to 3.08 bar absolute against engine speed, peaking near 1100 rpm. "
        "The manual describes the wastegate, the boost pressure positioner and the "
        "charge-air cooler, but publishes NO boost pressure. The 2.8 bar the "
        "turbocharger section mentions is the control pressure applied to the "
        "wastegate vacuum cell, NOT boost, and is deliberately not used here.",
        "The ECU boost setpoint the wastegate controller regulates towards. In "
        "Milestone 2 this was written straight into manifold pressure; boost is now "
        "an outcome of the compressor, and this is the target it is driven to.",
        0.0,
        1000000.0,
    ),
    cal(
        "air_path.intake_manifold_volume_m3",
        "0.020 m^3 for the charge-air housing and intake manifold together. "
        + NOT_PUBLISHED,
        "Control volume for the intake filling dynamics; sets how fast boost can "
        "build once the compressor delivers.",
        0.001,
        0.2,
    ),
    cal(
        "air_path.exhaust_manifold_volume_m3",
        "0.010 m^3 upstream of the turbine. " + NOT_PUBLISHED,
        "Control volume for the exhaust filling dynamics; sets how quickly blowdown "
        "energy reaches the turbine.",
        0.001,
        0.2,
    ),
    cal(
        "air_path.intercooler_effectiveness",
        "0.80 charge-air cooler effectiveness. The manual describes the cooler and "
        "its purpose but publishes no duty.",
        "Sets charge density after compression, and so the trapped air mass.",
        0.0,
        1.0,
    ),
    cal(
        "air_path.coolant_temperature_k",
        "358.15 K (85 C) coolant. " + NOT_PUBLISHED,
        "Heat sink for both the charge-air cooler and the EGR cooler.",
        330.0,
        380.0,
    ),
    cal(
        "air_path.exhaust_restriction_pa_per_kg2_s2",
        "120000 Pa per (kg/s)^2 downstream of the turbine, about 15 kPa at rated "
        "flow. " + NOT_PUBLISHED,
        "Lumped muffler and aftertreatment-can flow resistance. A restriction only: "
        "no filter loading, SCR, dosing or regeneration is modelled.",
        0.0,
        1000000.0,
    ),
    cal(
        "air_path.crankcase_pressure_pa",
        "101325 Pa on the underside of the piston.",
        "Back-pressure reference so crank torque uses the net gas force.",
        90000.0,
        115000.0,
    ),
    cal(
        "air_path.volumetric_efficiency",
        "0.92 trapping efficiency at intake valve closing. " + NOT_PUBLISHED,
        "Scales trapped fresh air, which sets the smoke-limited fuelling ceiling.",
        0.6,
        1.1,
    ),
    # --- turbocharger --------------------------------------------------------
    #
    # The manual publishes the architecture and nothing numeric. Note that the
    # "up to 2.8 bar" in the turbocharger section is the pneumatic pressure on
    # the wastegate vacuum cell, not boost pressure; it is not used anywhere.
    cal(
        "turbo.shaft_inertia_kg_m2",
        "3.5e-5 kg m^2 for the rotating assembly. " + NOT_PUBLISHED,
        "Sets turbocharger lag. This replaces Milestone 2's prescribed boost "
        "response time with a physical cause.",
        1e-06,
        0.001,
    ),
    cal(
        "turbo.compressor_wheel_diameter_m",
        "0.10 m compressor wheel. " + NOT_PUBLISHED,
        "With shaft speed this sets tip speed, and so the pressure ratio available.",
        0.02,
        0.25,
    ),
    cal(
        "turbo.compressor_efficiency",
        "0.72 isentropic. " + NOT_PUBLISHED,
        "Converts shaft work into pressure rise and sets compressor outlet "
        "temperature.",
        0.3,
        0.95,
    ),
    cal(
        "turbo.compressor_head_coefficient",
        "0.70 head coefficient. " + NOT_PUBLISHED,
        "Relates tip speed to the work the wheel does on the air.",
        0.1,
        1.5,
    ),
    cal(
        "turbo.compressor_max_flow_kg_s_per_rad_s",
        "6.5e-5 kg/s per rad/s of shaft speed. " + NOT_PUBLISHED,
        "Choke end of the simplified compressor map: the flow the wheel passes "
        "with no back-pressure.",
        1e-06,
        0.001,
    ),
    cal(
        "turbo.turbine_effective_area_m2",
        "6.0e-4 m^2 effective area. " + NOT_PUBLISHED,
        "Turbine swallowing capacity. Smaller areas raise back-pressure and spool "
        "harder; this is the primary boost-curve tuning parameter.",
        1e-05,
        0.01,
    ),
    cal(
        "turbo.turbine_efficiency",
        "0.70 isentropic. " + NOT_PUBLISHED,
        "Converts exhaust enthalpy drop into shaft power.",
        0.3,
        0.95,
    ),
    cal(
        "turbo.bearing_friction_nm_per_rad_s",
        "1.0e-8 N m per rad/s. " + NOT_PUBLISHED,
        "Bearing drag, which stops a freewheeling shaft from coasting forever.",
        0.0,
        1e-05,
    ),
    cal(
        "turbo.max_shaft_speed_rad_per_s",
        "15000 rad/s, about 143000 rpm. The manual documents a turbocharger "
        "protection function and a turbine speed sensor for code M5Z, but "
        "publishes no speed.",
        "Overspeed guard on the shaft state, mirroring the published protection "
        "function.",
        1000.0,
        30000.0,
    ),
    cal(
        "turbo.wastegate_max_area_m2",
        "2.5e-3 m^2 fully open. " + NOT_PUBLISHED,
        "Bypass capacity: how much exhaust can be routed around the turbine, which "
        "sets how hard boost can be pulled back.",
        1e-05,
        0.01,
    ),
    cal(
        "turbo.wastegate_p_gain_per_pa",
        "1.5e-5 per Pa of boost error. " + NOT_PUBLISHED,
        "Proportional term of the boost controller the MCM implements.",
        0.0,
        0.01,
    ),
    cal(
        "turbo.wastegate_i_gain_per_pa_s",
        "3.0e-5 per Pa-second. " + NOT_PUBLISHED,
        "Integral term removing steady-state boost error.",
        0.0,
        0.01,
    ),
    cal(
        "turbo.wastegate_slew_per_s",
        "6.0 per second, about a sixth of a second from shut to fully open. " + NOT_PUBLISHED,
        "Actuator rate limit: the vacuum cell and linkage cannot move instantly.",
        0.1,
        100.0,
    ),
    # --- exhaust gas recirculation -------------------------------------------
    pub(
        "egr.cooler_inlet_temperature_k",
        "923.15 K. The manual states the EGR cooler cools 'from a temperature of "
        "about 650 C'.",
        L_EGR,
    ),
    pub(
        "egr.cooler_outlet_temperature_k",
        "443.15 K. The manual states the EGR cooler cools 'down to about 170 C'.",
        L_EGR,
    ),
    der(
        "egr.cooler_effectiveness",
        f"{EGR_EFFECTIVENESS} from the published inlet and outlet temperatures.",
        "(T_in - T_out) / (T_in - T_coolant)",
        [
            "egr.cooler_inlet_temperature_k",
            "egr.cooler_outlet_temperature_k",
            "air_path.coolant_temperature_k",
        ],
    ),
    cal(
        "egr.rate_schedule",
        "0.04 to 0.14 recirculated mass fraction against engine speed, highest at "
        "low speed and tapering towards rated power. The manual states EGR is "
        "active across the whole speed range but publishes NO rate.",
        "Sets how much oxygen is displaced, which trades NOx against soot, power "
        "and fuel consumption.",
        0.0,
        1.0,
    ),
    cal(
        "egr.valve_max_area_m2",
        "4.0e-4 m^2 fully open. " + NOT_PUBLISHED,
        "Capacity of the throttle valve the EGR positioner drives.",
        1e-06,
        0.01,
    ),
    cal(
        "egr.valve_discharge_coefficient",
        "0.70. " + NOT_PUBLISHED,
        "Effective area of the valve relative to its geometric area.",
        0.1,
        1.0,
    ),
    cal(
        "egr.rate_p_gain",
        "20.0 per unit of rate error. " + NOT_PUBLISHED,
        "Proportional term of the recirculation rate controller.",
        0.0,
        1000.0,
    ),
    cal(
        "egr.rate_i_gain_per_s",
        "60.0 per unit-second. " + NOT_PUBLISHED,
        "Integral term holding the scheduled rate against changing pressures.",
        0.0,
        1000.0,
    ),
    cal(
        "egr.max_rate",
        "0.35 hard ceiling. " + NOT_PUBLISHED,
        "Above this the charge cannot burn cleanly; the manual notes soot, CO and "
        "HC rise when the exhaust proportion is too high.",
        0.0,
        0.5,
    ),
    # --- governor -----------------------------------------------------------
    pub("governor.idle_target_rpm", "560 rpm idle speed", L_TECH),
    cal(
        "governor.idle_p_gain_mg_per_rad_s",
        "1.2 mg per (rad/s) of speed error. " + NOT_PUBLISHED,
        "Proportional term of the idle governor. Requests fuel, never torque.",
        0.0,
        5.0,
    ),
    cal(
        "governor.idle_i_gain_mg_per_rad",
        "0.6 mg per (rad/s) per second. " + NOT_PUBLISHED,
        "Integral term holding the published 560 rpm idle against friction and "
        "accessory load.",
        0.0,
        50.0,
    ),
    cal(
        "governor.idle_integral_limit_mg",
        "40 mg integrator clamp.",
        "Prevents integral windup while cranking, which would overshoot idle.",
        1.0,
        350.0,
    ),
    cal(
        "governor.overspeed_taper_start_rpm",
        "1900 rpm. The manual publishes neither rated nor governed speed.",
        "Speed at which fuelling starts being withdrawn.",
        1000.0,
        2400.0,
    ),
    cal(
        "governor.overspeed_cutoff_rpm",
        "2100 rpm. The manual publishes neither rated nor governed speed.",
        "Speed at which fuelling reaches zero.",
        1100.0,
        2600.0,
    ),
    # --- load ---------------------------------------------------------------
    cal(
        "load.accessory_torque_constant_nm",
        "25 N m constant accessory drag. " + NOT_PUBLISHED,
        "Fan, alternator, compressor and pumps as an explicit resisting term.",
        0.0,
        200.0,
    ),
    cal(
        "load.accessory_torque_per_rad_s",
        "0.02 N m per (rad/s). " + NOT_PUBLISHED,
        "Speed-dependent part of the accessory drag.",
        0.0,
        1.0,
    ),
    cal(
        "load.max_external_load_nm",
        "5000 N m ceiling for the user-supplied and dynamometer load.",
        "Bounds the external load input; driveline behaviour arrives in Milestone 4.",
        100.0,
        20000.0,
    ),
    cal(
        "load.starter_torque_nm",
        "1500 N m at the crankshaft. " + NOT_PUBLISHED,
        "Cranks the engine through compression until fuelling takes over.",
        100.0,
        3000.0,
    ),
    cal(
        "load.starter_cutout_rpm",
        "300 rpm. " + NOT_PUBLISHED,
        "Speed at which starter torque reaches zero; also the running-state threshold.",
        100.0,
        600.0,
    ),
    # --- inertia ------------------------------------------------------------
    cal(
        "inertia.rotating_inertia_kg_m2",
        "3.5 kg m^2 for crankshaft, flywheel and rotating accessories. The manual "
        "publishes NO rotating inertia, and the published complete-engine mass is "
        "deliberately not used here.",
        "Sets crank acceleration for a given net torque.",
        0.5,
        20.0,
    ),
    pub(
        "inertia.complete_engine_mass_kg",
        "approx. 1200 kg complete-engine weight. Metadata only: never used as "
        "flywheel or rotating inertia.",
        L_TECH,
    ),
    # --- limits -------------------------------------------------------------
    pub(
        "limits.max_combustion_pressure_pa",
        "combustion pressures of up to 230 bar; used here as a validation and safety "
        "envelope, not as a pressure target",
        L_ENGINE,
    ),
    cal(
        "limits.max_rpm",
        "2400 rpm hard numerical guard, above the governed cut-out.",
        "Rejects a diverged solution rather than letting crank speed run away.",
        1200.0,
        3000.0,
    ),
    cal(
        "limits.max_gas_temperature_k",
        "3000 K hard numerical guard.",
        "Rejects a diverged single-zone gas state.",
        1500.0,
        4000.0,
    ),
    # --- solver -------------------------------------------------------------
    cal(
        "solver.fixed_step_s",
        "25 microseconds, about 0.36 degrees of crank rotation at 2400 rpm.",
        "Fixed integration step, small enough that the explicit crank and gas "
        "integration stays stable across the whole speed range.",
        0.000001,
        0.0001,
    ),
    cal(
        "solver.max_steps_per_batch",
        "20000 steps, i.e. 0.5 s of simulated time per advance call.",
        "Keeps the WASM boundary coarse while bounding the work of a single call.",
        1.0,
        200000.0,
    ),
    # --- audio --------------------------------------------------------------
    cal(
        "audio.reference_spl_db",
        "94 dB reference for the reported exhaust level. " + NOT_PUBLISHED,
        "Reference the level meter is quoted against. A display reference only: it "
        "is not a claim about how loud the real engine is.",
        60.0,
        120.0,
    ),
    cal(
        "audio.exhaust_gain",
        "12000. The acoustic source is a per-step difference of port flow, which "
        "is a small number, so the scaling is correspondingly large. " + NOT_PUBLISHED,
        "Brings the radiated exhaust signal to a usable level before the soft "
        "clipper. Set so the loudest realistic condition — the start transient, "
        "which is louder than full load — approaches the knee without sitting on "
        "it. Driving it far past the knee flat-tops the waveform, and a "
        "flat-topped wave is full of high harmonics: an audible buzz, not a "
        "louder engine.",
        0.0,
        1000000.0,
    ),
    cal(
        "audio.highpass_cutoff_hz",
        "20 Hz. " + NOT_PUBLISHED,
        "Removes the standing pressure offset so only the pulses remain, and keeps "
        "the signal from drifting off centre.",
        1.0,
        200.0,
    ),
    cal(
        "audio.lowpass_cutoff_hz",
        "6000 Hz, two poles. " + NOT_PUBLISHED,
        "Muffler roll-off. Without it the blowdown edge is far harsher than any "
        "real exhaust system sounds.",
        100.0,
        19000.0,
    ),
    cal(
        "audio.soft_clip_knee",
        "0.85. " + NOT_PUBLISHED,
        "Saturation ceiling of the soft clipper, which is what guarantees samples "
        "stay inside [-1, 1] without hard clipping.",
        0.1,
        1.0,
    ),
    # --- rated --------------------------------------------------------------
    pub(
        "rated.max_power_w",
        "375 kW for power code M3D. The engine speed at which this occurs is NOT "
        "published; the speed this model reaches it at is our calibration.",
        L_TECH + ", Power categories",
    ),
    pub(
        "rated.max_power_hp",
        "510 horsepower for power code M3D",
        L_TECH + ", Power categories",
    ),
    pub(
        "rated.max_torque_nm",
        "2500 N m for power code M3D. The engine speed at which this occurs is NOT "
        "published; the speed this model reaches it at is our calibration.",
        L_TECH + ", Power categories",
    ),
]

config["provenance"] = provenance

OUT.write_text(json.dumps(config, indent=2) + "\n")
print(f"wrote {OUT} with {len(provenance)} provenance entries")
