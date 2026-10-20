use serde::{Deserialize, Serialize};

use crate::{MachineType, ProjectError};

/// Flow through the machine at the design point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FlowRate {
    /// Volume flow in m³/s (for compressors: referred to the inlet total state).
    Volume(f64),
    /// Mass flow in kg/s.
    Mass(f64),
}

/// How the energy exchanged with the fluid is specified at the design point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EnergyTransfer {
    /// Head in m.
    Head(f64),
    /// Total-to-total pressure difference in Pa.
    TotalPressureDifference(f64),
    /// Total-to-total pressure ratio, always ≥ 1 (outlet/inlet for compressors,
    /// inlet/outlet for turbines).
    TotalPressureRatio(f64),
    /// Inlet total to outlet static pressure ratio, ≥ 1 (turbines).
    TotalToStaticPressureRatio(f64),
    /// Specific work in J/kg.
    SpecificWork(f64),
}

impl EnergyTransfer {
    pub fn value(&self) -> f64 {
        match *self {
            EnergyTransfer::Head(v)
            | EnergyTransfer::TotalPressureDifference(v)
            | EnergyTransfer::TotalPressureRatio(v)
            | EnergyTransfer::TotalToStaticPressureRatio(v)
            | EnergyTransfer::SpecificWork(v) => v,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            EnergyTransfer::Head(_) => "head",
            EnergyTransfer::TotalPressureDifference(_) => "total pressure difference",
            EnergyTransfer::TotalPressureRatio(_) => "total pressure ratio",
            EnergyTransfer::TotalToStaticPressureRatio(_) => "total-to-static pressure ratio",
            EnergyTransfer::SpecificWork(_) => "specific work",
        }
    }

    fn is_ratio(&self) -> bool {
        matches!(
            self,
            EnergyTransfer::TotalPressureRatio(_) | EnergyTransfer::TotalToStaticPressureRatio(_)
        )
    }
}

/// Direction of rotation, seen from the inlet looking downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    Clockwise,
    CounterClockwise,
}

/// The operating point the machine is designed for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesignPoint {
    pub flow: FlowRate,
    pub energy: EnergyTransfer,
    /// Rotational speed in rad/s.
    pub speed: f64,
}

/// Total state at the machine inlet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InletConditions {
    /// Total pressure in Pa.
    pub total_pressure: f64,
    /// Total temperature in K.
    pub total_temperature: f64,
}

impl Default for InletConditions {
    fn default() -> Self {
        Self {
            total_pressure: 101_325.0,
            total_temperature: 293.15,
        }
    }
}

/// Settings shared by every component of the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalSetup {
    pub design_point: DesignPoint,
    pub inlet: InletConditions,
    /// Name of the working fluid in the fluid database.
    pub fluid: String,
    pub rotation: Rotation,
}

impl GlobalSetup {
    /// A setup with standard inlet conditions and the usual fluid for `machine`.
    pub fn new(machine: MachineType, design_point: DesignPoint) -> Self {
        let fluid = match machine {
            MachineType::Pump | MachineType::HydroTurbine => "Water",
            MachineType::Fan | MachineType::Compressor | MachineType::GasTurbine => "Air",
        };
        Self {
            design_point,
            inlet: InletConditions::default(),
            fluid: fluid.to_owned(),
            rotation: Rotation::default(),
        }
    }

    /// Checks that the design point uses quantities that make sense for `machine` and
    /// that every value is physically possible.
    pub fn validate(&self, machine: MachineType) -> Result<(), ProjectError> {
        let invalid = |msg: String| Err(ProjectError::InvalidSetup(msg));
        let dp = &self.design_point;

        if machine == MachineType::GasTurbine && matches!(dp.flow, FlowRate::Volume(_)) {
            return invalid("gas turbines are specified by mass flow".into());
        }
        let flow = match dp.flow {
            FlowRate::Volume(v) | FlowRate::Mass(v) => v,
        };
        if !(flow.is_finite() && flow > 0.0) {
            return invalid(format!("flow rate must be positive, got {flow}"));
        }

        use EnergyTransfer as E;
        let energy_ok = match machine {
            MachineType::Pump => matches!(dp.energy, E::Head(_) | E::TotalPressureDifference(_)),
            MachineType::Fan => matches!(dp.energy, E::TotalPressureDifference(_)),
            MachineType::Compressor => matches!(
                dp.energy,
                E::TotalPressureRatio(_) | E::TotalPressureDifference(_) | E::SpecificWork(_)
            ),
            MachineType::GasTurbine => dp.energy.is_ratio(),
            MachineType::HydroTurbine => matches!(dp.energy, E::Head(_)),
        };
        if !energy_ok {
            return invalid(format!(
                "a {machine} cannot be specified by {}",
                dp.energy.name()
            ));
        }
        let e = dp.energy.value();
        let min = if dp.energy.is_ratio() { 1.0 } else { 0.0 };
        if !(e.is_finite() && e > min) {
            return invalid(format!(
                "{} must be greater than {min}, got {e}",
                dp.energy.name()
            ));
        }

        if !(dp.speed.is_finite() && dp.speed > 0.0) {
            return invalid(format!(
                "rotational speed must be positive, got {}",
                dp.speed
            ));
        }
        if !(self.inlet.total_pressure.is_finite() && self.inlet.total_pressure > 0.0) {
            return invalid("inlet total pressure must be positive".into());
        }
        if !(self.inlet.total_temperature.is_finite() && self.inlet.total_temperature > 0.0) {
            return invalid("inlet total temperature must be positive".into());
        }
        if self.fluid.trim().is_empty() {
            return invalid("no fluid selected".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(flow: FlowRate, energy: EnergyTransfer) -> DesignPoint {
        DesignPoint {
            flow,
            energy,
            speed: 300.0,
        }
    }

    #[test]
    fn accepts_typical_design_points() {
        let cases = [
            (
                MachineType::Pump,
                FlowRate::Volume(0.03),
                EnergyTransfer::Head(30.0),
            ),
            (
                MachineType::Fan,
                FlowRate::Volume(2.0),
                EnergyTransfer::TotalPressureDifference(800.0),
            ),
            (
                MachineType::Compressor,
                FlowRate::Mass(1.5),
                EnergyTransfer::TotalPressureRatio(3.2),
            ),
            (
                MachineType::GasTurbine,
                FlowRate::Mass(0.4),
                EnergyTransfer::TotalToStaticPressureRatio(2.5),
            ),
            (
                MachineType::HydroTurbine,
                FlowRate::Volume(5.0),
                EnergyTransfer::Head(120.0),
            ),
        ];
        for (machine, flow, energy) in cases {
            GlobalSetup::new(machine, point(flow, energy))
                .validate(machine)
                .unwrap_or_else(|e| panic!("{machine}: {e}"));
        }
    }

    #[test]
    fn rejects_quantities_foreign_to_the_machine() {
        let fan = GlobalSetup::new(
            MachineType::Fan,
            point(FlowRate::Volume(1.0), EnergyTransfer::Head(10.0)),
        );
        assert!(fan.validate(MachineType::Fan).is_err());

        let turbine = GlobalSetup::new(
            MachineType::GasTurbine,
            point(
                FlowRate::Volume(1.0),
                EnergyTransfer::TotalPressureRatio(2.0),
            ),
        );
        assert!(turbine.validate(MachineType::GasTurbine).is_err());
    }

    #[test]
    fn rejects_impossible_values() {
        let pump = |flow, head, speed| {
            let mut s = GlobalSetup::new(
                MachineType::Pump,
                point(FlowRate::Volume(flow), EnergyTransfer::Head(head)),
            );
            s.design_point.speed = speed;
            s.validate(MachineType::Pump)
        };
        assert!(pump(0.0, 30.0, 300.0).is_err());
        assert!(pump(0.03, -1.0, 300.0).is_err());
        assert!(pump(0.03, 30.0, f64::NAN).is_err());

        let compressor = GlobalSetup::new(
            MachineType::Compressor,
            point(FlowRate::Mass(1.0), EnergyTransfer::TotalPressureRatio(0.9)),
        );
        assert!(compressor.validate(MachineType::Compressor).is_err());
    }
}
