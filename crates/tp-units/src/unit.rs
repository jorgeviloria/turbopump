use crate::{QuantityKind, UnitError};

/// A unit of measurement related to SI by `si = value * factor + offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// Canonical symbol used for display, e.g. `"m³/h"`.
    pub symbol: &'static str,
    /// Alternative spellings accepted when parsing, e.g. `"m3/h"`.
    pub aliases: &'static [&'static str],
    pub kind: QuantityKind,
    pub factor: f64,
    pub offset: f64,
}

impl Unit {
    const fn new(
        kind: QuantityKind,
        symbol: &'static str,
        aliases: &'static [&'static str],
        factor: f64,
    ) -> Self {
        Self::affine(kind, symbol, aliases, factor, 0.0)
    }

    const fn affine(
        kind: QuantityKind,
        symbol: &'static str,
        aliases: &'static [&'static str],
        factor: f64,
        offset: f64,
    ) -> Self {
        Self {
            symbol,
            aliases,
            kind,
            factor,
            offset,
        }
    }

    /// Converts a value expressed in this unit to SI.
    pub fn to_si(&self, value: f64) -> f64 {
        value * self.factor + self.offset
    }

    /// Converts an SI value to this unit.
    pub fn from_si(&self, si: f64) -> f64 {
        (si - self.offset) / self.factor
    }

    /// Whether `text` is this unit's symbol or one of its aliases.
    pub fn matches(&self, text: &str) -> bool {
        self.symbol == text || self.aliases.contains(&text)
    }
}

/// Looks up a unit of the given kind by symbol or alias.
pub fn find_unit(kind: QuantityKind, symbol: &str) -> Result<&'static Unit, UnitError> {
    kind.units()
        .iter()
        .find(|u| u.matches(symbol))
        .ok_or_else(|| UnitError::UnknownUnit {
            symbol: symbol.to_owned(),
            kind,
            known: kind
                .units()
                .iter()
                .map(|u| u.symbol)
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// Converts `value` between two units of the same kind, given by symbol.
///
/// ```
/// let bar = tp_units::convert(150.0, "psi", "bar").unwrap();
/// assert!((bar - 10.342).abs() < 1e-3);
/// ```
pub fn convert(value: f64, from: &str, to: &str) -> Result<f64, UnitError> {
    let mut from_known = false;
    let mut to_known = false;
    for kind in QuantityKind::ALL {
        let f = kind.units().iter().find(|u| u.matches(from));
        let t = kind.units().iter().find(|u| u.matches(to));
        from_known |= f.is_some();
        to_known |= t.is_some();
        if let (Some(f), Some(t)) = (f, t) {
            return Ok(t.from_si(f.to_si(value)));
        }
    }
    if !from_known {
        Err(UnitError::UnknownSymbol(from.to_owned()))
    } else if !to_known {
        Err(UnitError::UnknownSymbol(to.to_owned()))
    } else {
        Err(UnitError::Incompatible {
            from: from.to_owned(),
            to: to.to_owned(),
        })
    }
}

/// Exact conversion factors (NIST SP 811, Appendix B).
mod factors {
    use std::f64::consts::PI;

    pub const INCH: f64 = 0.0254;
    pub const FOOT: f64 = 0.3048;
    pub const POUND_MASS: f64 = 0.453_592_37;
    pub const POUND_FORCE: f64 = 4.448_221_615_260_5;
    pub const US_GALLON: f64 = 3.785_411_784e-3;
    pub const CUBIC_FOOT: f64 = FOOT * FOOT * FOOT;
    pub const PSI: f64 = POUND_FORCE / (INCH * INCH);
    pub const HORSEPOWER: f64 = 550.0 * FOOT * POUND_FORCE;
    pub const BTU_IT_PER_LB: f64 = 2326.0;
    /// Conventional water column: ρ = 1000 kg/m³ and standard gravity.
    pub const MM_WATER: f64 = 1e-3 * 1000.0 * crate::STANDARD_GRAVITY;
    pub const IN_WATER: f64 = INCH * 1000.0 * crate::STANDARD_GRAVITY;
    pub const RPM: f64 = 2.0 * PI / 60.0;
    pub const DEGREE: f64 = PI / 180.0;
}

pub(crate) mod tables {
    use super::Unit;
    use super::factors::*;
    use crate::QuantityKind::*;

    pub static DIMENSIONLESS: &[Unit] = &[
        Unit::new(Dimensionless, "-", &["", "1"], 1.0),
        Unit::new(Dimensionless, "%", &[], 0.01),
    ];

    pub static LENGTH: &[Unit] = &[
        Unit::new(Length, "m", &[], 1.0),
        Unit::new(Length, "cm", &[], 1e-2),
        Unit::new(Length, "mm", &[], 1e-3),
        Unit::new(Length, "µm", &["um"], 1e-6),
        Unit::new(Length, "in", &["\""], INCH),
        Unit::new(Length, "ft", &["'"], FOOT),
    ];

    pub static HEAD: &[Unit] = &[
        Unit::new(Head, "m", &[], 1.0),
        Unit::new(Head, "ft", &[], FOOT),
    ];

    pub static AREA: &[Unit] = &[
        Unit::new(Area, "m²", &["m2", "m^2"], 1.0),
        Unit::new(Area, "cm²", &["cm2", "cm^2"], 1e-4),
        Unit::new(Area, "mm²", &["mm2", "mm^2"], 1e-6),
        Unit::new(Area, "in²", &["in2", "in^2"], INCH * INCH),
        Unit::new(Area, "ft²", &["ft2", "ft^2"], FOOT * FOOT),
    ];

    pub static ANGLE: &[Unit] = &[
        Unit::new(Angle, "rad", &[], 1.0),
        Unit::new(Angle, "°", &["deg"], DEGREE),
    ];

    pub static VELOCITY: &[Unit] = &[
        Unit::new(Velocity, "m/s", &[], 1.0),
        Unit::new(Velocity, "km/h", &[], 1.0 / 3.6),
        Unit::new(Velocity, "ft/s", &["fps"], FOOT),
        Unit::new(Velocity, "ft/min", &["fpm"], FOOT / 60.0),
    ];

    pub static ROTATIONAL_SPEED: &[Unit] = &[
        Unit::new(RotationalSpeed, "rad/s", &[], 1.0),
        Unit::new(
            RotationalSpeed,
            "rpm",
            &["RPM", "1/min", "min^-1", "r/min"],
            RPM,
        ),
        Unit::new(
            RotationalSpeed,
            "rev/s",
            &["rps"],
            2.0 * std::f64::consts::PI,
        ),
    ];

    pub static VOLUME_FLOW: &[Unit] = &[
        Unit::new(VolumeFlow, "m³/s", &["m3/s"], 1.0),
        Unit::new(VolumeFlow, "m³/h", &["m3/h"], 1.0 / 3600.0),
        Unit::new(VolumeFlow, "m³/min", &["m3/min"], 1.0 / 60.0),
        Unit::new(VolumeFlow, "l/s", &["L/s"], 1e-3),
        Unit::new(VolumeFlow, "l/min", &["L/min"], 1e-3 / 60.0),
        Unit::new(VolumeFlow, "gpm", &["USgpm", "gal/min"], US_GALLON / 60.0),
        Unit::new(
            VolumeFlow,
            "cfm",
            &["ft3/min", "ft³/min"],
            CUBIC_FOOT / 60.0,
        ),
        Unit::new(VolumeFlow, "ft³/s", &["ft3/s", "cfs"], CUBIC_FOOT),
    ];

    pub static MASS_FLOW: &[Unit] = &[
        Unit::new(MassFlow, "kg/s", &[], 1.0),
        Unit::new(MassFlow, "kg/h", &[], 1.0 / 3600.0),
        Unit::new(MassFlow, "t/h", &[], 1000.0 / 3600.0),
        Unit::new(MassFlow, "lb/s", &["lbm/s"], POUND_MASS),
        Unit::new(MassFlow, "lb/min", &["lbm/min"], POUND_MASS / 60.0),
        Unit::new(MassFlow, "lb/h", &["lbm/h"], POUND_MASS / 3600.0),
    ];

    pub static PRESSURE: &[Unit] = &[
        Unit::new(Pressure, "Pa", &[], 1.0),
        Unit::new(Pressure, "hPa", &[], 1e2),
        Unit::new(Pressure, "kPa", &[], 1e3),
        Unit::new(Pressure, "MPa", &[], 1e6),
        Unit::new(Pressure, "N/mm²", &["N/mm2"], 1e6),
        Unit::new(Pressure, "bar", &[], 1e5),
        Unit::new(Pressure, "mbar", &[], 1e2),
        Unit::new(Pressure, "atm", &[], 101_325.0),
        Unit::new(Pressure, "psi", &["lbf/in2", "lbf/in²"], PSI),
        Unit::new(Pressure, "mmH₂O", &["mmH2O", "mmWC"], MM_WATER),
        Unit::new(Pressure, "inH₂O", &["inH2O", "inWC"], IN_WATER),
    ];

    pub static TEMPERATURE: &[Unit] = &[
        Unit::new(Temperature, "K", &[], 1.0),
        Unit::affine(Temperature, "°C", &["degC", "C"], 1.0, 273.15),
        Unit::affine(
            Temperature,
            "°F",
            &["degF", "F"],
            5.0 / 9.0,
            459.67 * 5.0 / 9.0,
        ),
        Unit::new(Temperature, "°R", &["degR", "R"], 5.0 / 9.0),
    ];

    pub static DENSITY: &[Unit] = &[
        Unit::new(Density, "kg/m³", &["kg/m3"], 1.0),
        Unit::new(Density, "g/cm³", &["g/cm3"], 1000.0),
        Unit::new(
            Density,
            "lb/ft³",
            &["lb/ft3", "lbm/ft3"],
            POUND_MASS / CUBIC_FOOT,
        ),
    ];

    pub static DYNAMIC_VISCOSITY: &[Unit] = &[
        Unit::new(DynamicViscosity, "Pa·s", &["Pa.s", "Pas", "Pa s"], 1.0),
        Unit::new(DynamicViscosity, "mPa·s", &["mPa.s", "mPas", "cP"], 1e-3),
        Unit::new(
            DynamicViscosity,
            "lb/(ft·s)",
            &["lb/(ft.s)", "lb/ft/s"],
            POUND_MASS / FOOT,
        ),
    ];

    pub static KINEMATIC_VISCOSITY: &[Unit] = &[
        Unit::new(KinematicViscosity, "m²/s", &["m2/s"], 1.0),
        Unit::new(KinematicViscosity, "mm²/s", &["mm2/s", "cSt"], 1e-6),
        Unit::new(KinematicViscosity, "ft²/s", &["ft2/s"], FOOT * FOOT),
    ];

    pub static POWER: &[Unit] = &[
        Unit::new(Power, "W", &[], 1.0),
        Unit::new(Power, "kW", &[], 1e3),
        Unit::new(Power, "MW", &[], 1e6),
        Unit::new(Power, "hp", &["HP"], HORSEPOWER),
    ];

    pub static TORQUE: &[Unit] = &[
        Unit::new(Torque, "N·m", &["N.m", "Nm", "N m"], 1.0),
        Unit::new(Torque, "kN·m", &["kN.m", "kNm"], 1e3),
        Unit::new(
            Torque,
            "lbf·ft",
            &["lbf.ft", "lbf ft", "ft.lbf"],
            POUND_FORCE * FOOT,
        ),
    ];

    pub static FORCE: &[Unit] = &[
        Unit::new(Force, "N", &[], 1.0),
        Unit::new(Force, "kN", &[], 1e3),
        Unit::new(Force, "lbf", &[], POUND_FORCE),
    ];

    pub static SPECIFIC_ENERGY: &[Unit] = &[
        Unit::new(SpecificEnergy, "J/kg", &["m2/s2", "m²/s²"], 1.0),
        Unit::new(SpecificEnergy, "kJ/kg", &[], 1e3),
        Unit::new(SpecificEnergy, "Btu/lb", &["BTU/lb"], BTU_IT_PER_LB),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn first_unit_of_every_kind_is_si() {
        for kind in QuantityKind::ALL {
            let si = kind.si_unit();
            assert_eq!(si.factor, 1.0, "{kind}");
            assert_eq!(si.offset, 0.0, "{kind}");
            assert_eq!(si.kind, kind);
        }
    }

    #[test]
    fn symbols_are_unique_within_a_kind() {
        for kind in QuantityKind::ALL {
            let spellings: Vec<&str> = kind
                .units()
                .iter()
                .flat_map(|u| std::iter::once(u.symbol).chain(u.aliases.iter().copied()))
                .collect();
            for (i, s) in spellings.iter().enumerate() {
                assert!(!spellings[i + 1..].contains(s), "duplicate {s:?} in {kind}");
            }
        }
    }

    #[test]
    fn units_are_tagged_with_their_own_kind() {
        for kind in QuantityKind::ALL {
            for u in kind.units() {
                assert_eq!(u.kind, kind, "{}", u.symbol);
            }
        }
    }

    #[test]
    fn known_conversions() {
        assert_relative_eq!(convert(1.0, "in", "mm").unwrap(), 25.4);
        assert_relative_eq!(
            convert(1.0, "m3/s", "gpm").unwrap(),
            15_850.323,
            epsilon = 1e-3
        );
        assert_relative_eq!(
            convert(1.0, "psi", "Pa").unwrap(),
            6_894.757_293,
            epsilon = 1e-6
        );
        assert_relative_eq!(
            convert(1.0, "hp", "W").unwrap(),
            745.699_872,
            epsilon = 1e-6
        );
        assert_relative_eq!(
            convert(1.0, "lb/ft3", "kg/m3").unwrap(),
            16.018_463,
            epsilon = 1e-6
        );
        assert_relative_eq!(
            convert(1.0, "inH2O", "Pa").unwrap(),
            249.088_91,
            epsilon = 1e-5
        );
        assert_relative_eq!(
            convert(1450.0, "rpm", "rad/s").unwrap(),
            151.843_644,
            epsilon = 1e-6
        );
        assert_relative_eq!(convert(180.0, "deg", "rad").unwrap(), std::f64::consts::PI);
    }

    #[test]
    fn temperatures_use_offsets() {
        assert_relative_eq!(convert(0.0, "°C", "K").unwrap(), 273.15);
        assert_relative_eq!(convert(212.0, "°F", "°C").unwrap(), 100.0, epsilon = 1e-12);
        assert_relative_eq!(convert(-40.0, "°F", "°C").unwrap(), -40.0, epsilon = 1e-12);
        assert_relative_eq!(convert(491.67, "°R", "K").unwrap(), 273.15, epsilon = 1e-12);
    }

    #[test]
    fn conversion_errors() {
        assert_eq!(
            convert(1.0, "bar", "rpm"),
            Err(UnitError::Incompatible {
                from: "bar".into(),
                to: "rpm".into()
            })
        );
        assert_eq!(
            convert(1.0, "furlong", "m"),
            Err(UnitError::UnknownSymbol("furlong".into()))
        );
        assert!(matches!(
            find_unit(QuantityKind::Pressure, "rpm"),
            Err(UnitError::UnknownUnit { .. })
        ));
    }

    proptest::proptest! {
        #[test]
        fn si_round_trip(value in -1e9f64..1e9) {
            for kind in QuantityKind::ALL {
                for u in kind.units() {
                    let back = u.from_si(u.to_si(value));
                    proptest::prop_assert!((back - value).abs() <= 1e-9 * value.abs().max(1.0));
                }
            }
        }
    }
}
