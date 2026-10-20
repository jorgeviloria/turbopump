use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Unit;
use crate::unit::tables;

/// The physical meaning of a value, which decides the units it can be shown in.
///
/// `Head` and `Length` share the dimension metre but are separate kinds, because users
/// expect head in m or ft and geometry in mm or in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityKind {
    Dimensionless,
    Length,
    Head,
    Area,
    Angle,
    Velocity,
    RotationalSpeed,
    VolumeFlow,
    MassFlow,
    Pressure,
    Temperature,
    Density,
    DynamicViscosity,
    KinematicViscosity,
    Power,
    Torque,
    Force,
    SpecificEnergy,
}

impl QuantityKind {
    /// Every kind, in declaration order.
    pub const ALL: [QuantityKind; 18] = [
        QuantityKind::Dimensionless,
        QuantityKind::Length,
        QuantityKind::Head,
        QuantityKind::Area,
        QuantityKind::Angle,
        QuantityKind::Velocity,
        QuantityKind::RotationalSpeed,
        QuantityKind::VolumeFlow,
        QuantityKind::MassFlow,
        QuantityKind::Pressure,
        QuantityKind::Temperature,
        QuantityKind::Density,
        QuantityKind::DynamicViscosity,
        QuantityKind::KinematicViscosity,
        QuantityKind::Power,
        QuantityKind::Torque,
        QuantityKind::Force,
        QuantityKind::SpecificEnergy,
    ];

    /// Human-readable name, lower case.
    pub fn name(self) -> &'static str {
        match self {
            QuantityKind::Dimensionless => "dimensionless",
            QuantityKind::Length => "length",
            QuantityKind::Head => "head",
            QuantityKind::Area => "area",
            QuantityKind::Angle => "angle",
            QuantityKind::Velocity => "velocity",
            QuantityKind::RotationalSpeed => "rotational speed",
            QuantityKind::VolumeFlow => "volume flow",
            QuantityKind::MassFlow => "mass flow",
            QuantityKind::Pressure => "pressure",
            QuantityKind::Temperature => "temperature",
            QuantityKind::Density => "density",
            QuantityKind::DynamicViscosity => "dynamic viscosity",
            QuantityKind::KinematicViscosity => "kinematic viscosity",
            QuantityKind::Power => "power",
            QuantityKind::Torque => "torque",
            QuantityKind::Force => "force",
            QuantityKind::SpecificEnergy => "specific energy",
        }
    }

    /// All units of this kind. The first entry is the SI unit.
    pub fn units(self) -> &'static [Unit] {
        match self {
            QuantityKind::Dimensionless => tables::DIMENSIONLESS,
            QuantityKind::Length => tables::LENGTH,
            QuantityKind::Head => tables::HEAD,
            QuantityKind::Area => tables::AREA,
            QuantityKind::Angle => tables::ANGLE,
            QuantityKind::Velocity => tables::VELOCITY,
            QuantityKind::RotationalSpeed => tables::ROTATIONAL_SPEED,
            QuantityKind::VolumeFlow => tables::VOLUME_FLOW,
            QuantityKind::MassFlow => tables::MASS_FLOW,
            QuantityKind::Pressure => tables::PRESSURE,
            QuantityKind::Temperature => tables::TEMPERATURE,
            QuantityKind::Density => tables::DENSITY,
            QuantityKind::DynamicViscosity => tables::DYNAMIC_VISCOSITY,
            QuantityKind::KinematicViscosity => tables::KINEMATIC_VISCOSITY,
            QuantityKind::Power => tables::POWER,
            QuantityKind::Torque => tables::TORQUE,
            QuantityKind::Force => tables::FORCE,
            QuantityKind::SpecificEnergy => tables::SPECIFIC_ENERGY,
        }
    }

    /// The SI unit of this kind (factor 1, no offset).
    pub fn si_unit(self) -> &'static Unit {
        &self.units()[0]
    }
}

impl fmt::Display for QuantityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
