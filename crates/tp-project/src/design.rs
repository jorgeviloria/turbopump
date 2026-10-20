//! Running design calculations on the components of a project.

use tp_design::MainDimensionsInput;
use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometry, ImpellerGeometryInput};
use tp_design::pump::{self, CentrifugalPumpDesign, CentrifugalPumpInput, PumpDuty};
use tp_fluids::LiquidProperties;
use tp_units::STANDARD_GRAVITY;

use crate::{
    ComponentId, ComponentKind, DesignStep, EnergyTransfer, FlowRate, MachineType, Project,
    ProjectError, Rotation,
};

/// Design steps completed by [`Project::design_impeller_geometry`], in order.
pub const GEOMETRY_STEPS: [DesignStep; 5] = [
    DesignStep::MeridionalContour,
    DesignStep::BladeProperties,
    DesignStep::BladeMeanLines,
    DesignStep::BladeProfiles,
    DesignStep::BladeEdges,
];

impl Project {
    /// Properties of the working fluid at the inlet total state.
    pub fn liquid(&self) -> Result<LiquidProperties, ProjectError> {
        let setup = self.setup();
        Ok(tp_fluids::liquid_properties(
            &setup.fluid,
            setup.inlet.total_pressure,
            setup.inlet.total_temperature,
        )?)
    }

    fn impeller_count(&self) -> usize {
        self.components()
            .iter()
            .filter(|c| c.kind() == ComponentKind::Impeller)
            .count()
    }

    /// Operating point of impeller `id` in a pump project. The head is split equally
    /// between the impellers of a multistage pump.
    pub fn pump_duty(&self, id: ComponentId) -> Result<PumpDuty, ProjectError> {
        if self.machine() != MachineType::Pump {
            return Err(ProjectError::Unsupported(format!(
                "main dimensions of {} impellers (planned for phase 6)",
                self.machine()
            )));
        }
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        if component.kind() != ComponentKind::Impeller {
            return Err(ProjectError::NotAnImpeller(id));
        }
        let liquid = self.liquid()?;
        let setup = self.setup();
        let dp = &setup.design_point;
        let flow = match dp.flow {
            FlowRate::Volume(q) => q,
            FlowRate::Mass(m) => m / liquid.density,
        };
        let machine_head = match dp.energy {
            EnergyTransfer::Head(h) => h,
            EnergyTransfer::TotalPressureDifference(p) => p / (liquid.density * STANDARD_GRAVITY),
            other => {
                return Err(ProjectError::InvalidSetup(format!(
                    "a pump cannot be specified by {}",
                    other.name()
                )));
            }
        };
        Ok(PumpDuty {
            flow,
            head: machine_head / self.impeller_count() as f64,
            machine_head,
            speed: dp.speed,
            density: liquid.density,
            vapor_pressure: liquid.vapor_pressure,
            inlet_total_pressure: setup.inlet.total_pressure,
        })
    }

    /// The main-dimensions inputs of impeller `id`, all automatic if never set.
    pub fn pump_impeller_input(
        &self,
        id: ComponentId,
    ) -> Result<CentrifugalPumpInput, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        Ok(match component.main_dimensions() {
            Some(MainDimensionsInput::CentrifugalPump(input)) => input.clone(),
            None => CentrifugalPumpInput::default(),
        })
    }

    /// Computes the main dimensions of impeller `id` without changing the project.
    pub fn compute_main_dimensions(
        &self,
        id: ComponentId,
    ) -> Result<CentrifugalPumpDesign, ProjectError> {
        let duty = self.pump_duty(id)?;
        let input = self.pump_impeller_input(id)?;
        Ok(pump::design(&duty, &input)?)
    }

    /// Computes the main dimensions of impeller `id` and marks the step as done.
    pub fn design_main_dimensions(
        &mut self,
        id: ComponentId,
    ) -> Result<CentrifugalPumpDesign, ProjectError> {
        let result = self.compute_main_dimensions(id)?;
        self.component_mut(id)?
            .complete(DesignStep::MainDimensions)?;
        Ok(result)
    }
}

impl Project {
    /// The geometry inputs of impeller `id`, all automatic if never set.
    pub fn impeller_geometry_input(
        &self,
        id: ComponentId,
    ) -> Result<ImpellerGeometryInput, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        Ok(component.geometry().cloned().unwrap_or_default())
    }

    /// Builds the meridional contour, meridional flow and blades of impeller `id` from
    /// its main dimensions, without changing the project.
    pub fn compute_impeller_geometry(
        &self,
        id: ComponentId,
    ) -> Result<ImpellerGeometry, ProjectError> {
        let main = self.compute_main_dimensions(id)?;
        let main_input = self.pump_impeller_input(id)?;
        let sign = match self.setup().rotation {
            Rotation::Clockwise => 1.0,
            Rotation::CounterClockwise => -1.0,
        };
        let basis = ImpellerBasis::from_pump(&main, &main_input, sign);
        Ok(impeller::build(&basis, &self.impeller_geometry_input(id)?)?)
    }

    /// Builds the geometry of impeller `id` and marks the geometry steps as done.
    /// The main dimensions must be done first.
    pub fn design_impeller_geometry(
        &mut self,
        id: ComponentId,
    ) -> Result<ImpellerGeometry, ProjectError> {
        let geometry = self.compute_impeller_geometry(id)?;
        let component = self.component_mut(id)?;
        for step in GEOMETRY_STEPS {
            component.complete(step)?;
        }
        Ok(geometry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::pump;
    use crate::{GlobalSetup, StepStatus};
    use approx::assert_relative_eq;
    use tp_design::Param;

    #[test]
    fn designs_the_impeller_of_a_pump_project() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let result = p.design_main_dimensions(id).unwrap();
        assert_relative_eq!(result.nq, 35.92, epsilon = 0.01);
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::MainDimensions),
            StepStatus::Done
        );
    }

    #[test]
    fn changing_inputs_makes_the_step_stale() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.design_main_dimensions(id).unwrap();
        let input = CentrifugalPumpInput {
            blade_count: Param::Manual(5),
            ..Default::default()
        };
        p.component_mut(id)
            .unwrap()
            .set_main_dimensions(MainDimensionsInput::CentrifugalPump(input))
            .unwrap();
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::MainDimensions),
            StepStatus::Stale
        );
        assert_eq!(p.design_main_dimensions(id).unwrap().blade_count, 5);

        // The inputs survive a save and load.
        let loaded = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(
            loaded.pump_impeller_input(id).unwrap().blade_count,
            Param::Manual(5)
        );
    }

    #[test]
    fn multistage_pumps_split_the_head() {
        let mut p = pump();
        let first = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        let duty = p.pump_duty(first).unwrap();
        assert_relative_eq!(duty.head, 16.0);
        assert_relative_eq!(duty.machine_head, 32.0);
    }

    #[test]
    fn pressure_and_mass_flow_are_converted_with_the_density() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let rho = p.liquid().unwrap().density;
        let mut setup: GlobalSetup = p.setup().clone();
        setup.design_point.flow = FlowRate::Mass(rho * 0.02);
        setup.design_point.energy = EnergyTransfer::TotalPressureDifference(rho * 9.80665 * 30.0);
        p.set_setup(setup).unwrap();
        let duty = p.pump_duty(id).unwrap();
        assert_relative_eq!(duty.flow, 0.02, max_relative = 1e-12);
        assert_relative_eq!(duty.head, 30.0, max_relative = 1e-12);
    }

    #[test]
    fn only_pump_impellers_are_supported_for_now() {
        let mut p = pump();
        let volute = p.add_component(ComponentKind::Volute, None).unwrap();
        assert!(matches!(
            p.pump_duty(volute),
            Err(ProjectError::NotAnImpeller(_))
        ));
        assert!(matches!(
            p.component_mut(volute).unwrap().set_main_dimensions(
                MainDimensionsInput::CentrifugalPump(CentrifugalPumpInput::default())
            ),
            Err(ProjectError::NotAnImpeller(_))
        ));
    }

    #[test]
    fn geometry_requires_main_dimensions() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        assert!(matches!(
            p.design_impeller_geometry(id),
            Err(ProjectError::PreviousStepIncomplete { .. })
        ));
        p.design_main_dimensions(id).unwrap();
        let g = p.design_impeller_geometry(id).unwrap();
        assert_eq!(g.blade.rotation_sign, 1.0);
        let c = p.component(id).unwrap();
        for step in GEOMETRY_STEPS {
            assert_eq!(c.status(step), StepStatus::Done);
        }
        assert_eq!(c.next_step(), Some(DesignStep::CfdSetup));
    }

    #[test]
    fn geometry_changes_invalidate_from_the_changed_step() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.design_main_dimensions(id).unwrap();
        p.design_impeller_geometry(id).unwrap();

        let mut input = p.impeller_geometry_input(id).unwrap();
        input.edges.trailing = tp_design::impeller::blade::EdgeShape::Ellipse { ratio: 1.0 };
        p.component_mut(id)
            .unwrap()
            .set_geometry(input.clone())
            .unwrap();
        let c = p.component(id).unwrap();
        assert_eq!(c.status(DesignStep::BladeProfiles), StepStatus::Done);
        assert_eq!(c.status(DesignStep::BladeEdges), StepStatus::Stale);

        input.meridional.leading_edge_hub = Param::Manual(0.3);
        p.component_mut(id).unwrap().set_geometry(input).unwrap();
        let c = p.component(id).unwrap();
        assert_eq!(c.status(DesignStep::MainDimensions), StepStatus::Done);
        assert_eq!(c.status(DesignStep::MeridionalContour), StepStatus::Stale);

        // Inputs survive saving.
        let loaded = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(
            loaded
                .impeller_geometry_input(id)
                .unwrap()
                .meridional
                .leading_edge_hub,
            Param::Manual(0.3)
        );
    }

    #[test]
    fn counter_clockwise_rotation_mirrors_the_blades() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let mut setup = p.setup().clone();
        setup.rotation = Rotation::CounterClockwise;
        p.set_setup(setup).unwrap();
        assert_eq!(
            p.compute_impeller_geometry(id).unwrap().blade.rotation_sign,
            -1.0
        );
    }

    #[test]
    fn unknown_fluid_is_reported() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let mut setup = p.setup().clone();
        setup.fluid = "Unobtainium".into();
        p.set_setup(setup).unwrap();
        assert!(matches!(
            p.compute_main_dimensions(id),
            Err(ProjectError::Fluid(_))
        ));
    }
}
