use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The kind of machine a project describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineType {
    Pump,
    Fan,
    Compressor,
    GasTurbine,
    HydroTurbine,
}

impl MachineType {
    pub const ALL: [MachineType; 5] = [
        MachineType::Pump,
        MachineType::Fan,
        MachineType::Compressor,
        MachineType::GasTurbine,
        MachineType::HydroTurbine,
    ];

    /// Identifier used in files and on the command line.
    pub fn id(self) -> &'static str {
        match self {
            MachineType::Pump => "pump",
            MachineType::Fan => "fan",
            MachineType::Compressor => "compressor",
            MachineType::GasTurbine => "gas_turbine",
            MachineType::HydroTurbine => "hydro_turbine",
        }
    }

    /// Whether density changes through the machine must be modelled.
    pub fn is_compressible(self) -> bool {
        matches!(self, MachineType::Compressor | MachineType::GasTurbine)
    }

    /// Whether the machine extracts energy from the fluid rather than adding it.
    pub fn is_turbine(self) -> bool {
        matches!(self, MachineType::GasTurbine | MachineType::HydroTurbine)
    }

    /// Maximum number of impellers (runners) in one project.
    pub fn max_impellers(self) -> usize {
        match self {
            MachineType::HydroTurbine => 1,
            _ => 5,
        }
    }
}

impl fmt::Display for MachineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MachineType::Pump => "pump",
            MachineType::Fan => "fan",
            MachineType::Compressor => "compressor",
            MachineType::GasTurbine => "gas turbine",
            MachineType::HydroTurbine => "hydro turbine",
        })
    }
}

impl FromStr for MachineType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MachineType::ALL
            .into_iter()
            .find(|m| m.id() == s || m.id().replace('_', "-") == s)
            .ok_or_else(|| {
                let ids: Vec<_> = MachineType::ALL.iter().map(|m| m.id()).collect();
                format!(
                    "unknown machine type {s:?}; expected one of {}",
                    ids.join(", ")
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for m in MachineType::ALL {
            assert_eq!(m.id().parse::<MachineType>().unwrap(), m);
        }
        assert_eq!(
            "gas-turbine".parse::<MachineType>().unwrap(),
            MachineType::GasTurbine
        );
        assert!("windmill".parse::<MachineType>().is_err());
    }
}
