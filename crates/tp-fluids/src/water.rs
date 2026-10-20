//! Built-in model of liquid water.
//!
//! Saturation pressure and saturated-liquid density use the IAPWS 1992 supplementary
//! equations, valid from the triple point to the critical point:
//! W. Wagner, A. Pruss, "International equations for the saturation properties of
//! ordinary water substance. Revised according to the international temperature scale
//! of 1990", *J. Phys. Chem. Ref. Data* 22 (1993) 783–787.
//!
//! Viscosity uses a Vogel equation, μ = A·exp(B / (T − C)), fitted to water
//! (D. S. Viswanath, G. Natarajan, *Data Book on the Viscosity of Liquids*, Hemisphere
//! 1989). The tests check it against the IAPWS 2008 reference values within 1.2 %
//! over 0–100 °C.
//!
//! Density is taken as that of saturated liquid. Compressing water by 1 MPa raises
//! its density by only about 0.05 %, which is negligible for pump design.

use crate::{FluidError, LiquidProperties, PropertySource};

const CRITICAL_TEMPERATURE: f64 = 647.096;
const CRITICAL_PRESSURE: f64 = 22.064e6;
const CRITICAL_DENSITY: f64 = 322.0;

/// Temperature range (K) accepted by the built-in model: triple point to 150 °C.
/// The viscosity fit is not checked above that.
pub const BUILTIN_WATER_RANGE: (f64, f64) = (273.16, 423.15);

/// Saturation (vapour) pressure of water in Pa at temperature `t` in K.
pub fn saturation_pressure(t: f64) -> f64 {
    const A: [f64; 6] = [
        -7.859_517_83,
        1.844_082_59,
        -11.786_649_7,
        22.680_741_1,
        -15.961_871_9,
        1.801_225_02,
    ];
    let tau = 1.0 - t / CRITICAL_TEMPERATURE;
    let sum = A[0] * tau
        + A[1] * tau.powf(1.5)
        + A[2] * tau.powi(3)
        + A[3] * tau.powf(3.5)
        + A[4] * tau.powi(4)
        + A[5] * tau.powf(7.5);
    CRITICAL_PRESSURE * (CRITICAL_TEMPERATURE / t * sum).exp()
}

/// Density of saturated liquid water in kg/m³ at temperature `t` in K.
pub fn saturated_liquid_density(t: f64) -> f64 {
    const B: [f64; 6] = [
        1.992_740_64,
        1.099_653_42,
        -0.510_839_303,
        -1.754_934_79,
        -45.517_035_2,
        -6.746_944_50e5,
    ];
    const EXPONENTS: [f64; 6] = [
        1.0 / 3.0,
        2.0 / 3.0,
        5.0 / 3.0,
        16.0 / 3.0,
        43.0 / 3.0,
        110.0 / 3.0,
    ];
    let tau = 1.0 - t / CRITICAL_TEMPERATURE;
    let sum: f64 = B.iter().zip(EXPONENTS).map(|(b, e)| b * tau.powf(e)).sum();
    CRITICAL_DENSITY * (1.0 + sum)
}

/// Dynamic viscosity of liquid water in Pa·s at temperature `t` in K.
pub fn water_viscosity(t: f64) -> f64 {
    const A: f64 = 0.029_39e-3;
    const B: f64 = 507.88;
    const C: f64 = 149.3;
    A * (B / (t - C)).exp()
}

pub(crate) fn liquid_properties(
    pressure: f64,
    temperature: f64,
) -> Result<LiquidProperties, FluidError> {
    let (min, max) = BUILTIN_WATER_RANGE;
    if !(min..=max).contains(&temperature) {
        return Err(FluidError::OutOfRange {
            model: "the built-in water model",
            temperature,
            min,
            max,
        });
    }
    let vapor_pressure = saturation_pressure(temperature);
    if pressure <= vapor_pressure {
        return Err(FluidError::NotLiquid {
            fluid: "Water".into(),
            pressure,
            temperature,
            vapor_pressure,
        });
    }
    Ok(LiquidProperties {
        density: saturated_liquid_density(temperature),
        dynamic_viscosity: water_viscosity(temperature),
        vapor_pressure,
        source: PropertySource::BuiltinWater,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const C0: f64 = 273.15;

    #[test]
    fn saturation_pressure_matches_iapws() {
        // IAPWS-95 saturation table.
        assert_relative_eq!(saturation_pressure(C0 + 20.0), 2_339.2, max_relative = 1e-4);
        assert_relative_eq!(
            saturation_pressure(C0 + 60.0),
            19_946.0,
            max_relative = 1e-4
        );
        assert_relative_eq!(
            saturation_pressure(C0 + 100.0),
            101_418.0,
            max_relative = 1e-4
        );
    }

    #[test]
    fn saturated_density_matches_iapws() {
        assert_relative_eq!(
            saturated_liquid_density(C0 + 20.0),
            998.16,
            max_relative = 1e-4
        );
        assert_relative_eq!(
            saturated_liquid_density(C0 + 60.0),
            983.16,
            max_relative = 1e-4
        );
        assert_relative_eq!(
            saturated_liquid_density(C0 + 100.0),
            958.35,
            max_relative = 1e-4
        );
    }

    #[test]
    fn viscosity_within_fit_tolerance() {
        // IAPWS 2008 at 0.1 MPa (100 °C: saturated liquid), mPa·s.
        let reference = [
            (0.0, 1.7914),
            (20.0, 1.0016),
            (40.0, 0.6527),
            (60.0, 0.4665),
            (80.0, 0.3544),
            (100.0, 0.2818),
        ];
        for (celsius, mu) in reference {
            assert_relative_eq!(
                water_viscosity(C0 + celsius) * 1e3,
                mu,
                max_relative = 0.012
            );
        }
    }

    #[test]
    fn rejects_vapour_and_out_of_range_states() {
        assert!(matches!(
            liquid_properties(50_000.0, C0 + 90.0),
            Err(FluidError::NotLiquid { .. })
        ));
        assert!(matches!(
            liquid_properties(1e6, C0 + 200.0),
            Err(FluidError::OutOfRange { .. })
        ));
        let ok = liquid_properties(101_325.0, C0 + 20.0).unwrap();
        assert_relative_eq!(ok.kinematic_viscosity(), 1.004e-6, max_relative = 0.01);
    }
}
