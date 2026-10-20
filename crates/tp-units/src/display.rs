use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{QuantityKind, Unit, UnitError, find_unit};

/// The unit chosen for showing and entering each kind of quantity.
///
/// Serialized as a map from kind to unit symbol. Kinds missing from a saved map fall
/// back to the SI preset, so files written by older versions stay readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "BTreeMap<QuantityKind, String>",
    into = "BTreeMap<QuantityKind, String>"
)]
pub struct DisplayUnits {
    units: BTreeMap<QuantityKind, &'static Unit>,
}

impl DisplayUnits {
    /// Metric engineering units: mm, m³/h, bar, °C, kW, rpm...
    pub fn si() -> Self {
        use QuantityKind::*;
        Self::from_symbols(&[
            (Dimensionless, "-"),
            (Length, "mm"),
            (Head, "m"),
            (Area, "mm²"),
            (Angle, "°"),
            (Velocity, "m/s"),
            (RotationalSpeed, "rpm"),
            (VolumeFlow, "m³/h"),
            (MassFlow, "kg/s"),
            (Pressure, "bar"),
            (Temperature, "°C"),
            (Density, "kg/m³"),
            (DynamicViscosity, "mPa·s"),
            (KinematicViscosity, "mm²/s"),
            (Power, "kW"),
            (Torque, "N·m"),
            (Force, "N"),
            (SpecificEnergy, "J/kg"),
        ])
    }

    /// US customary units: in, gpm, ft, psi, °F, hp, rpm...
    pub fn us() -> Self {
        use QuantityKind::*;
        Self::from_symbols(&[
            (Dimensionless, "-"),
            (Length, "in"),
            (Head, "ft"),
            (Area, "in²"),
            (Angle, "°"),
            (Velocity, "ft/s"),
            (RotationalSpeed, "rpm"),
            (VolumeFlow, "gpm"),
            (MassFlow, "lb/s"),
            (Pressure, "psi"),
            (Temperature, "°F"),
            (Density, "lb/ft³"),
            (DynamicViscosity, "mPa·s"),
            (KinematicViscosity, "mm²/s"),
            (Power, "hp"),
            (Torque, "lbf·ft"),
            (Force, "lbf"),
            (SpecificEnergy, "Btu/lb"),
        ])
    }

    fn from_symbols(pairs: &[(QuantityKind, &str)]) -> Self {
        let units = pairs
            .iter()
            .map(|&(kind, symbol)| {
                let unit = find_unit(kind, symbol).expect("preset units exist in the tables");
                (kind, unit)
            })
            .collect();
        Self { units }
    }

    /// The unit used for `kind`.
    pub fn get(&self, kind: QuantityKind) -> &'static Unit {
        self.units
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.si_unit())
    }

    /// Chooses the unit used for `kind`.
    pub fn set(&mut self, kind: QuantityKind, symbol: &str) -> Result<(), UnitError> {
        self.units.insert(kind, find_unit(kind, symbol)?);
        Ok(())
    }

    /// Converts an SI value to the display unit of `kind`.
    pub fn to_display(&self, kind: QuantityKind, si: f64) -> f64 {
        self.get(kind).from_si(si)
    }

    /// Converts a value in the display unit of `kind` to SI.
    pub fn from_display(&self, kind: QuantityKind, value: f64) -> f64 {
        self.get(kind).to_si(value)
    }

    /// Formats an SI value in the display unit, e.g. `"100.0 m³/h"`.
    pub fn format(&self, kind: QuantityKind, si: f64, decimals: usize) -> String {
        let unit = self.get(kind);
        let value = unit.from_si(si);
        if unit.symbol == "-" {
            format!("{value:.decimals$}")
        } else {
            format!("{value:.decimals$} {}", unit.symbol)
        }
    }
}

impl Default for DisplayUnits {
    fn default() -> Self {
        Self::si()
    }
}

impl TryFrom<BTreeMap<QuantityKind, String>> for DisplayUnits {
    type Error = UnitError;

    fn try_from(map: BTreeMap<QuantityKind, String>) -> Result<Self, Self::Error> {
        let mut units = Self::si();
        for (kind, symbol) in map {
            units.set(kind, &symbol)?;
        }
        Ok(units)
    }
}

impl From<DisplayUnits> for BTreeMap<QuantityKind, String> {
    fn from(units: DisplayUnits) -> Self {
        units
            .units
            .into_iter()
            .map(|(kind, unit)| (kind, unit.symbol.to_owned()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn presets_cover_every_kind() {
        for units in [DisplayUnits::si(), DisplayUnits::us()] {
            for kind in QuantityKind::ALL {
                assert!(units.units.contains_key(&kind), "{kind}");
            }
        }
    }

    #[test]
    fn formats_in_display_unit() {
        let si = DisplayUnits::si();
        assert_eq!(
            si.format(QuantityKind::VolumeFlow, 100.0 / 3600.0, 1),
            "100.0 m³/h"
        );
        assert_eq!(si.format(QuantityKind::Dimensionless, 0.5, 2), "0.50");
        let us = DisplayUnits::us();
        assert_eq!(us.format(QuantityKind::Head, 0.3048, 2), "1.00 ft");
    }

    #[test]
    fn display_round_trip() {
        let us = DisplayUnits::us();
        let si = us.from_display(QuantityKind::Temperature, 68.0);
        assert_relative_eq!(si, 293.15, epsilon = 1e-12);
        assert_relative_eq!(
            us.to_display(QuantityKind::Temperature, si),
            68.0,
            epsilon = 1e-12
        );
    }

    #[test]
    fn serde_round_trip_and_partial_maps() {
        let mut units = DisplayUnits::si();
        units.set(QuantityKind::Pressure, "kPa").unwrap();
        let json = serde_json::to_string(&units).unwrap();
        let back: DisplayUnits = serde_json::from_str(&json).unwrap();
        assert_eq!(back, units);

        let partial: DisplayUnits = serde_json::from_str(r#"{"head":"ft"}"#).unwrap();
        assert_eq!(partial.get(QuantityKind::Head).symbol, "ft");
        assert_eq!(partial.get(QuantityKind::Pressure).symbol, "bar");

        let bad = serde_json::from_str::<DisplayUnits>(r#"{"head":"bar"}"#);
        assert!(bad.is_err());
    }
}
