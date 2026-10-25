//! Running design calculations on the components of a project.

use tp_design::MainDimensionsInput;
use tp_design::diffuser::{DiffuserDesign, DiffuserInput};
use tp_design::impeller::duct::{self, DuctInput, InletDuct};
use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometry, ImpellerGeometryInput};
use tp_design::npsh::CollectorThroat;
use tp_design::outlet::ImpellerOutlet;
use tp_design::performance::{self, Collector, CollectorData, PerformanceInput, PumpPerformance};
use tp_design::pump::{self, CentrifugalPumpDesign, CentrifugalPumpInput, PumpDuty, Suction};
use tp_design::volute::loss::VoluteLossGeometry;
use tp_design::volute::{VoluteDesign, VoluteGeometry, VoluteInput};
use tp_fluids::LiquidProperties;
use tp_units::STANDARD_GRAVITY;

use crate::{
    ComponentId, ComponentKind, DesignStep, EnergyTransfer, FlowRate, MachineType, Project,
    ProjectError, Rotation, StepStatus,
};

/// Design steps completed by [`Project::design_impeller_geometry`], in order.
pub const GEOMETRY_STEPS: [DesignStep; 5] = [
    DesignStep::MeridionalContour,
    DesignStep::BladeProperties,
    DesignStep::BladeMeanLines,
    DesignStep::BladeProfiles,
    DesignStep::BladeEdges,
];

/// Design steps of a vaneless stator that the program can compute, in order.
pub const VANELESS_STEPS: [DesignStep; 2] =
    [DesignStep::MainDimensions, DesignStep::MeridionalContour];

/// Design steps of a vaned stator (radial diffuser) that the program can compute.
pub const VANED_STEPS: [DesignStep; 6] = [
    DesignStep::MainDimensions,
    DesignStep::MeridionalContour,
    DesignStep::BladeProperties,
    DesignStep::BladeMeanLines,
    DesignStep::BladeProfiles,
    DesignStep::BladeEdges,
];

/// Design steps of a volute that the program can compute, in order.
pub const VOLUTE_STEPS: [DesignStep; 6] = [
    DesignStep::VoluteSetup,
    DesignStep::CrossSection,
    DesignStep::SpiralDevelopment,
    DesignStep::Diffuser,
    DesignStep::CutWater,
    DesignStep::WallThickness,
];

/// What [`Project::design_remaining`] did.
#[derive(Debug)]
pub struct Designed {
    /// The steps done, in order.
    pub done: Vec<DesignStep>,
    /// The step that could not be done, and why.
    pub failed: Option<(DesignStep, ProjectError)>,
}

/// Whether the program computes step `step` of a component of `kind`; the others
/// are planned for later phases.
pub fn can_design(kind: ComponentKind, step: DesignStep) -> bool {
    match kind {
        ComponentKind::Impeller => {
            step == DesignStep::MainDimensions || GEOMETRY_STEPS.contains(&step)
        }
        ComponentKind::Volute => VOLUTE_STEPS.contains(&step),
        ComponentKind::Stator { vaned: true } => VANED_STEPS.contains(&step),
        ComponentKind::Stator { vaned: false } => VANELESS_STEPS.contains(&step),
        ComponentKind::InletDuct => step == DesignStep::DuctDimensions,
    }
}

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
            stages: self.impeller_count() as u32,
            speed: dp.speed,
            density: liquid.density,
            vapor_pressure: liquid.vapor_pressure,
            inlet_total_pressure: setup.inlet.total_pressure,
            kinematic_viscosity: liquid.kinematic_viscosity(),
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

    /// What collects the flow after impeller `id`: the component that follows it, or a
    /// volute when nothing does.
    pub fn collector_after(&self, id: ComponentId) -> Result<Collector, ProjectError> {
        let components = self.components();
        let index = components
            .iter()
            .position(|c| c.id() == id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        Ok(match components.get(index + 1).map(|c| c.kind()) {
            Some(ComponentKind::Stator { vaned: true }) => Collector::VanedDiffuser,
            Some(ComponentKind::Stator { vaned: false }) => Collector::VanelessDiffuser,
            _ => Collector::Volute,
        })
    }

    /// Default settings of the performance prediction of impeller `id`.
    pub fn performance_input(&self, id: ComponentId) -> Result<PerformanceInput, ProjectError> {
        Ok(PerformanceInput {
            collector: self.collector_after(id)?,
            ..PerformanceInput::default()
        })
    }

    /// What the performance prediction of impeller `id` needs from the collector after
    /// it: the throat of the volute or of the vaned diffuser, and the volute for its
    /// losses when it follows the impeller directly or behind a vaneless diffuser.
    pub fn collector_data_after(&self, id: ComponentId) -> Result<CollectorData, ProjectError> {
        let index = self.index_of(id)?;
        let components = self.components();
        let volute = |at: usize| -> Result<CollectorData, ProjectError> {
            let id = components[at].id();
            let (basis, input) = (self.volute_basis(id)?, self.volute_input(id)?);
            // A flange height fixes the length of the diffuser through the geometry.
            let design = if input.diffuser.flange_height.manual_value().is_some() {
                VoluteGeometry::build(&basis, &input)?.design
            } else {
                VoluteDesign::new(&basis, &input)?
            };
            // A double volute has two throats; their mean stands for both.
            let throat = match &design.rib {
                Some(rib) => CollectorThroat {
                    count: 2,
                    area: 0.5 * (design.throat.area + rib.outer_throat.area),
                },
                None => CollectorThroat {
                    count: 1,
                    area: design.throat.area,
                },
            };
            Ok(CollectorData {
                throat: Some(throat),
                volute: Some(VoluteLossGeometry::from_design(&design)),
                double_volute_wrap: design.rib.map(|r| r.wrap),
                throat_integral: Some(design.throat_integral()?),
                cutwater: Some((design.cutwater_flow_angle, design.cutwater_angle)),
            })
        };
        let kind = |at: usize| components.get(at).map(|c| c.kind());
        Ok(match kind(index + 1) {
            Some(ComponentKind::Volute) => volute(index + 1)?,
            Some(ComponentKind::Stator { vaned: true }) => {
                let diffuser = self.compute_diffuser(components[index + 1].id())?;
                CollectorData {
                    throat: diffuser.vanes.as_ref().map(|v| CollectorThroat {
                        count: v.count,
                        area: v.throat * diffuser.inlet_width,
                    }),
                    volute: None,
                    double_volute_wrap: None,
                    throat_integral: None,
                    cutwater: None,
                }
            }
            Some(ComponentKind::Stator { vaned: false })
                if kind(index + 2) == Some(ComponentKind::Volute) =>
            {
                volute(index + 2)?
            }
            _ => CollectorData::default(),
        })
    }

    /// The performance input of impeller `id` with the seals of its meridional contour,
    /// wherever `input` leaves them automatic: the front annular seal (diameter,
    /// clearance and length) and the rear one for the balance holes. With the side
    /// chambers designed, the casing run measured on the front seat is its length, and
    /// the shaft seal and the axial gap come from the chambers too. Values set by hand
    /// in `input` stay.
    pub fn with_contour_seals(
        &self,
        id: ComponentId,
        input: &PerformanceInput,
    ) -> PerformanceInput {
        let mut input = input.clone();
        let Ok(geometry) = self.compute_impeller_geometry(id) else {
            return input;
        };
        let contour = &geometry.contour;
        let set = |param: &mut tp_design::Param<f64>, value: f64| {
            if param.is_auto() && value > 0.0 {
                *param = tp_design::Param::Manual(value);
            }
        };
        // Worn seals stay twice as wide, as the analysis takes them.
        let wear = if input.wide_seal_clearances { 2.0 } else { 1.0 };
        let chambers = &contour.chambers;
        let v = chambers.values;
        if let Some(front) = &contour.seals.front {
            set(&mut input.axial.seal_diameter, 2.0 * front.radius);
            set(&mut input.losses.seal_clearance, wear * front.clearance);
            // The measured run can be shorter than the nominal length.
            let length = if chambers.enabled && v.seal_length > 0.0 {
                v.seal_length
            } else {
                front.length
            };
            set(&mut input.losses.seal_length, length);
        }
        if let Some(rear) = &contour.seals.rear {
            set(&mut input.axial.rear_seal_diameter, 2.0 * rear.radius);
        }
        if chambers.enabled {
            set(&mut input.axial.shaft_seal_diameter, v.shaft_seal_diameter);
            set(&mut input.losses.axial_gap, v.axial_gap);
        }
        input
    }

    /// Predicts the characteristic of impeller `id` from its main dimensions and the
    /// collector after it, with the seals of its meridional contour
    /// (see [`Project::with_contour_seals`]). A collector that cannot be computed is left
    /// out with a warning.
    pub fn compute_performance(
        &self,
        id: ComponentId,
        input: &PerformanceInput,
    ) -> Result<PumpPerformance, ProjectError> {
        let mut input = self.with_contour_seals(id, input);
        // The balance holes open into the rear seal's gap: without it they are left out.
        let no_rear_seal = input.axial.balance_holes
            && self
                .compute_impeller_geometry(id)
                .is_ok_and(|g| g.contour.seals.rear.is_none());
        if no_rear_seal {
            input.axial.balance_holes = false;
        }
        let input = &input;
        let duty = self.pump_duty(id)?;
        let design = self.compute_main_dimensions(id)?;
        let (collector, problem) = match self.collector_data_after(id) {
            Ok(c) => (c, None),
            Err(e) => (CollectorData::default(), Some(e)),
        };
        let mut curve = performance::predict(&duty, &design, input, &collector)?;
        if no_rear_seal {
            curve
                .warnings
                .push("balance holes need the rear seal; they are left out".to_string());
        }
        if let Some(e) = problem {
            curve.warnings.push(format!(
                "the collector after the impeller is left out of the prediction: {e}"
            ));
        }
        Ok(curve)
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
        Ok(impeller::build(
            &basis,
            &self.impeller_geometry_input(id)?,
            main_input.suction.eyes(),
        )?)
    }

    /// Every step of component `id` that is not done yet, computed in order with the
    /// current inputs (automatic values where nothing was set by hand). Stops at the
    /// first step that fails: the steps before it stay done.
    pub fn design_remaining(&mut self, id: ComponentId) -> Result<Designed, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        let kind = component.kind();
        let remaining: Vec<DesignStep> = kind
            .steps()
            .iter()
            .copied()
            .filter(|&s| can_design(kind, s) && component.status(s) != StepStatus::Done)
            .collect();
        let mut done = Vec::new();
        for step in remaining {
            if let Err(e) = self.design_step(id, step) {
                return Ok(Designed {
                    done,
                    failed: Some((step, e)),
                });
            }
            done.push(step);
        }
        Ok(Designed { done, failed: None })
    }

    /// Computes step `step` of component `id` with the current inputs and marks it
    /// done. Every earlier step must be done. Geometry and volute steps build the whole
    /// geometry, so they fail if any of its inputs is invalid.
    pub fn design_step(&mut self, id: ComponentId, step: DesignStep) -> Result<(), ProjectError> {
        let kind = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?
            .kind();
        match kind {
            ComponentKind::Impeller if step == DesignStep::MainDimensions => {
                self.design_main_dimensions(id)?;
            }
            ComponentKind::Impeller if GEOMETRY_STEPS.contains(&step) => {
                self.compute_impeller_geometry(id)?;
                self.component_mut(id)?.complete(step)?;
            }
            ComponentKind::Volute if VOLUTE_STEPS.contains(&step) => {
                self.compute_volute(id)?;
                self.component_mut(id)?.complete(step)?;
            }
            ComponentKind::Stator { vaned: false } if VANELESS_STEPS.contains(&step) => {
                self.compute_diffuser(id)?;
                self.component_mut(id)?.complete(step)?;
            }
            ComponentKind::Stator { vaned: true } if VANED_STEPS.contains(&step) => {
                self.compute_diffuser(id)?;
                self.component_mut(id)?.complete(step)?;
            }
            ComponentKind::InletDuct if step == DesignStep::DuctDimensions => {
                let built = self.compute_duct(id)?;
                if let Some(w) = built.warnings.first() {
                    return Err(ProjectError::Unsupported(w.clone()));
                }
                self.component_mut(id)?.complete(step)?;
            }
            _ => {
                return Err(ProjectError::Unsupported(format!(
                    "the {step} step of a {} (planned for a later phase)",
                    kind.label().to_lowercase()
                )));
            }
        }
        Ok(())
    }

    /// Marks the inlet duct ahead of impeller `id` stale: it is built on the inlet of
    /// the impeller.
    fn invalidate_duct_before(&mut self, id: ComponentId) -> Result<(), ProjectError> {
        let index = self.index_of(id)?;
        if let Some(i) = index.checked_sub(1)
            && self.components()[i].kind() == ComponentKind::InletDuct
        {
            let duct = self.components()[i].id();
            self.component_mut(duct)?.invalidate_all();
        }
        Ok(())
    }

    /// The inputs of inlet duct `id`, all automatic if never set.
    pub fn duct_input(&self, id: ComponentId) -> Result<DuctInput, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        if component.kind() != ComponentKind::InletDuct {
            return Err(ProjectError::NotAnInletDuct(id));
        }
        Ok(component.duct().copied().unwrap_or_default())
    }

    /// The impeller after inlet duct `id`, whose inlet the duct is built on.
    pub fn impeller_after_duct(&self, id: ComponentId) -> Result<ComponentId, ProjectError> {
        let index = self.index_of(id)?;
        match self.components().get(index + 1) {
            Some(c) if c.kind() == ComponentKind::Impeller => Ok(c.id()),
            _ => Err(ProjectError::DuctNotBeforeImpeller),
        }
    }

    /// Builds inlet duct `id` on the inlet of the impeller after it, without changing
    /// the project. What cannot be built as asked is in its warnings.
    pub fn compute_duct(&self, id: ComponentId) -> Result<InletDuct, ProjectError> {
        let input = self.duct_input(id)?;
        let g = self.compute_impeller_geometry(self.impeller_after_duct(id)?)?;
        Ok(duct::build(&g.contour, &g.basis, &input))
    }

    /// Replaces the inputs of inlet duct `id`.
    pub fn set_duct(&mut self, id: ComponentId, input: DuctInput) -> Result<(), ProjectError> {
        self.component_mut(id)?.set_duct(input)
    }

    /// Marks every component after `id` stale: they are coupled to its outlet.
    fn invalidate_downstream(&mut self, id: ComponentId) -> Result<(), ProjectError> {
        let index = self
            .components()
            .iter()
            .position(|c| c.id() == id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        let ids: Vec<ComponentId> = self.components()[index + 1..]
            .iter()
            .map(|c| c.id())
            .collect();
        for next in ids {
            self.component_mut(next)?.invalidate_all();
        }
        Ok(())
    }

    /// Replaces the main-dimensions inputs of impeller `id`. A change makes its whole
    /// design stale, and the components after it too.
    pub fn set_main_dimensions(
        &mut self,
        id: ComponentId,
        input: MainDimensionsInput,
    ) -> Result<(), ProjectError> {
        let changed = self.component(id).and_then(|c| c.main_dimensions()) != Some(&input);
        self.component_mut(id)?.set_main_dimensions(input)?;
        if changed {
            self.invalidate_downstream(id)?;
            self.invalidate_duct_before(id)?;
        }
        Ok(())
    }

    /// Replaces the geometry inputs of impeller `id`. A new meridional contour moves the
    /// outlet, so it also makes the components after it stale.
    pub fn set_impeller_geometry(
        &mut self,
        id: ComponentId,
        input: ImpellerGeometryInput,
    ) -> Result<(), ProjectError> {
        let old = self.impeller_geometry_input(id)?;
        let outlet_moved = old.meridional != input.meridional;
        self.component_mut(id)?.set_geometry(input)?;
        if outlet_moved {
            self.invalidate_downstream(id)?;
            // The inlet moves too.
            self.invalidate_duct_before(id)?;
        }
        Ok(())
    }

    /// Replaces the inputs of volute `id`.
    pub fn set_volute(&mut self, id: ComponentId, input: VoluteInput) -> Result<(), ProjectError> {
        self.component_mut(id)?.set_volute(input)
    }

    /// Replaces the diffuser inputs of stator `id`. Its outlet feeds the next component,
    /// so a change of the main dimensions or contour makes the components after it stale.
    pub fn set_diffuser(
        &mut self,
        id: ComponentId,
        input: DiffuserInput,
    ) -> Result<(), ProjectError> {
        let old = self.diffuser_input(id)?;
        let outlet_moved = old.main != input.main || old.contour != input.contour;
        self.component_mut(id)?.set_diffuser(input)?;
        if outlet_moved {
            self.invalidate_downstream(id)?;
        }
        Ok(())
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

impl Project {
    /// The volute inputs of `id`, all automatic if never set.
    pub fn volute_input(&self, id: ComponentId) -> Result<VoluteInput, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        if component.kind() != ComponentKind::Volute {
            return Err(ProjectError::NotAVolute(id));
        }
        Ok(component.volute().cloned().unwrap_or_default())
    }

    /// What the component at `index` receives from the impeller just before it.
    fn impeller_outlet_before(&self, index: usize) -> Result<ImpellerOutlet, ProjectError> {
        let component = &self.components()[index];
        let what = component.kind().label().to_lowercase();
        if index == 0 {
            return Err(ProjectError::Unsupported(format!(
                "a {what} at the inlet (planned for a later phase)"
            )));
        }
        let upstream = &self.components()[index - 1];
        match upstream.kind() {
            ComponentKind::Impeller => {}
            // A diffuser hands on its own outlet (Gülich Chap. 7.11).
            ComponentKind::Stator { vaned } => {
                let inlet = self.impeller_outlet_before(index - 1)?;
                let d = DiffuserDesign::new(&inlet, &self.diffuser_input(upstream.id())?, vaned)?;
                return Ok(ImpellerOutlet {
                    impeller_diameter: d.outlet_diameter,
                    impeller_width: d.outlet_width,
                    cu: d.outlet.cu,
                    cm: d.outlet.cm,
                    // The diffuser walls, not the impeller, stand beside what follows.
                    sides: None,
                    ..inlet
                });
            }
            ComponentKind::Volute => {
                return Err(ProjectError::Unsupported(format!(
                    "a {what} after a volute (planned for a later phase)"
                )));
            }
            ComponentKind::InletDuct => {
                return Err(ProjectError::Unsupported(format!(
                    "a {what} right after an inlet duct"
                )));
            }
        }
        let impeller = upstream.id();
        let main = self.compute_main_dimensions(impeller)?;
        let duty = self.pump_duty(impeller)?;
        let geometry = self.compute_impeller_geometry(impeller)?;
        // A double-entry impeller is modelled as one half whose hub is the plane of
        // symmetry; a single-entry impeller discharges between shroud and hub.
        // The hub end of the outlet, wherever the meridian is placed.
        let length = geometry.contour.hub.end().x;
        let midplane = match self.pump_impeller_input(impeller)?.suction {
            Suction::Single => length - geometry.basis.outlet_width / 2.0,
            Suction::Double => length,
        };
        // The faces of the impeller, with the walls of its CAD model, for a casing
        // beside it.
        let double_entry = self.pump_impeller_input(impeller)?.suction == Suction::Double;
        let wall = geometry.contour.material.wall_thickness;
        Ok(ImpellerOutlet {
            sides: Some(geometry.sides(wall, double_entry)),
            ..ImpellerOutlet::from_pump(&main, &duty, midplane, geometry.blade.rotation_sign)
        })
    }

    fn index_of(&self, id: ComponentId) -> Result<usize, ProjectError> {
        self.components()
            .iter()
            .position(|c| c.id() == id)
            .ok_or(ProjectError::UnknownComponent(id))
    }

    /// What volute `id` receives from the impeller just upstream.
    pub fn volute_basis(&self, id: ComponentId) -> Result<ImpellerOutlet, ProjectError> {
        let index = self.index_of(id)?;
        if self.components()[index].kind() != ComponentKind::Volute {
            return Err(ProjectError::NotAVolute(id));
        }
        self.impeller_outlet_before(index)
    }

    /// Builds volute `id` without changing the project.
    pub fn compute_volute(&self, id: ComponentId) -> Result<VoluteGeometry, ProjectError> {
        let basis = self.volute_basis(id)?;
        Ok(VoluteGeometry::build(&basis, &self.volute_input(id)?)?)
    }
}

impl Project {
    /// The diffuser inputs of stator `id`, all automatic if never set.
    pub fn diffuser_input(&self, id: ComponentId) -> Result<DiffuserInput, ProjectError> {
        let component = self
            .component(id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        if !matches!(component.kind(), ComponentKind::Stator { .. }) {
            return Err(ProjectError::NotAStator(id));
        }
        Ok(component.diffuser().cloned().unwrap_or_default())
    }

    /// Designs the diffuser of stator `id` from the impeller before it, without
    /// changing the project.
    pub fn compute_diffuser(&self, id: ComponentId) -> Result<DiffuserDesign, ProjectError> {
        let index = self.index_of(id)?;
        let ComponentKind::Stator { vaned } = self.components()[index].kind() else {
            return Err(ProjectError::NotAStator(id));
        };
        let outlet = self.impeller_outlet_before(index)?;
        Ok(DiffuserDesign::new(
            &outlet,
            &self.diffuser_input(id)?,
            vaned,
        )?)
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
    fn the_remaining_steps_are_designed_in_one_go() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let volute = p.add_component(ComponentKind::Volute, None).unwrap();
        let Designed { done, failed } = p.design_remaining(id).unwrap();
        assert!(failed.is_none());
        assert_eq!(done[0], DesignStep::MainDimensions);
        assert_eq!(done[1..], GEOMETRY_STEPS);
        // Nothing left: the planned steps are not attempted.
        let Designed { done, failed } = p.design_remaining(id).unwrap();
        assert!(done.is_empty() && failed.is_none());
        let Designed { done, failed } = p.design_remaining(volute).unwrap();
        assert!(failed.is_none());
        assert_eq!(done, VOLUTE_STEPS);
    }

    #[test]
    fn designing_the_remaining_steps_stops_at_the_first_failure() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.design_step(id, DesignStep::MainDimensions).unwrap();
        let mut input = p.impeller_geometry_input(id).unwrap();
        input.properties.spans = Param::Manual(1);
        p.component_mut(id).unwrap().set_geometry(input).unwrap();
        let Designed { done, failed } = p.design_remaining(id).unwrap();
        assert!(done.is_empty());
        assert_eq!(failed.unwrap().0, DesignStep::MeridionalContour);
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::MainDimensions),
            StepStatus::Done,
            "what was done stays done"
        );
    }

    #[test]
    fn steps_are_designed_one_at_a_time() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        assert!(matches!(
            p.design_step(id, DesignStep::BladeProperties),
            Err(ProjectError::PreviousStepIncomplete { .. })
        ));
        for step in [DesignStep::MainDimensions, DesignStep::MeridionalContour] {
            p.design_step(id, step).unwrap();
        }
        let c = p.component(id).unwrap();
        assert_eq!(c.next_step(), Some(DesignStep::BladeProperties));
        assert!(matches!(
            p.design_step(id, DesignStep::CfdSetup),
            Err(ProjectError::Unsupported(_))
        ));

        // Invalid geometry inputs keep the step from completing.
        let mut input = p.impeller_geometry_input(id).unwrap();
        input.properties.spans = Param::Manual(1);
        p.component_mut(id).unwrap().set_geometry(input).unwrap();
        assert!(p.design_step(id, DesignStep::BladeProperties).is_err());
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::BladeProperties),
            StepStatus::Pending
        );
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

    fn pump_with_volute() -> (Project, ComponentId, ComponentId) {
        let mut p = pump();
        let impeller = p.add_component(ComponentKind::Impeller, None).unwrap();
        let volute = p.add_component(ComponentKind::Volute, None).unwrap();
        (p, impeller, volute)
    }

    #[test]
    fn an_inlet_duct_comes_first_and_follows_the_impeller_inlet() {
        let (mut p, impeller, volute) = pump_with_volute();
        // Only first, and only before an impeller.
        assert!(matches!(
            p.add_component(ComponentKind::InletDuct, Some(1)),
            Err(ProjectError::DuctNotBeforeImpeller)
        ));
        let duct = p.add_component(ComponentKind::InletDuct, Some(0)).unwrap();
        assert_eq!(p.components()[0].name, "Inlet duct 1");
        assert!(matches!(
            p.add_component(ComponentKind::InletDuct, Some(0)),
            Err(ProjectError::DuctNotBeforeImpeller)
        ));
        assert!(matches!(
            p.remove_component(impeller),
            Err(ProjectError::DuctNotBeforeImpeller)
        ));
        // Built on the inlet of the impeller after it, with its eye as run-up.
        let built = p.compute_duct(duct).unwrap();
        let g = p.compute_impeller_geometry(impeller).unwrap();
        assert!(built.warnings.is_empty(), "{:?}", built.warnings);
        assert_eq!(built.values.length, g.basis.inlet_diameter);
        p.design_step(duct, DesignStep::DuctDimensions).unwrap();
        assert!(p.component(duct).unwrap().is_complete());
        // A new contour moves the inlet: the duct is outdated, the volute too.
        p.design_step(volute, DesignStep::VoluteSetup).unwrap();
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.axial_length = Param::Manual(0.05);
        p.set_impeller_geometry(impeller, input).unwrap();
        for id in [duct, volute] {
            assert!(
                p.component(id)
                    .unwrap()
                    .steps()
                    .any(|(_, s)| s == StepStatus::Stale)
            );
        }
        // Its own inputs outdate it, and a value it cannot build fails the step.
        p.design_step(duct, DesignStep::DuctDimensions).unwrap();
        let bad = DuctInput {
            length: Param::Manual(-0.1),
            ..DuctInput::default()
        };
        p.set_duct(duct, bad).unwrap();
        assert_eq!(
            p.component(duct)
                .unwrap()
                .status(DesignStep::DuctDimensions),
            StepStatus::Stale
        );
        assert!(p.design_step(duct, DesignStep::DuctDimensions).is_err());
        assert!(matches!(
            p.set_duct(impeller, DuctInput::default()),
            Err(ProjectError::NotAnInletDuct(_))
        ));
    }

    #[test]
    fn older_files_turn_a_modelled_duct_into_a_component() {
        let (mut p, impeller, _) = pump_with_volute();
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.axial_length = Param::Manual(0.05);
        p.set_impeller_geometry(impeller, input).unwrap();
        // As older versions wrote it: the duct inside the contour, with its switch.
        let mut json: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        json["project"]["components"][0]["geometry"]["meridional"]["inlet_duct"] =
            serde_json::json!({ "enabled": true, "length": { "mode": "manual", "value": 0.12 } });
        let old = Project::from_json(&json.to_string()).unwrap();
        let kinds: Vec<ComponentKind> = old.components().iter().map(|c| c.kind()).collect();
        assert_eq!(
            kinds,
            [
                ComponentKind::InletDuct,
                ComponentKind::Impeller,
                ComponentKind::Volute
            ]
        );
        let duct = old.components()[0].id();
        assert_eq!(old.duct_input(duct).unwrap().length, Param::Manual(0.12));
        assert!(duct.0 > impeller.0);
        // The rest of the contour stays, the switch goes.
        let contour = old.impeller_geometry_input(impeller).unwrap().meridional;
        assert_eq!(contour.axial_length, Param::Manual(0.05));
        assert_eq!(contour.inlet_duct, Default::default());
        // A duct that was off leaves no component.
        json["project"]["components"][0]["geometry"]["meridional"]["inlet_duct"] =
            serde_json::json!({ "enabled": false });
        let off = Project::from_json(&json.to_string()).unwrap();
        assert_eq!(off.components().len(), 2);
    }

    #[test]
    fn designed_side_chambers_lend_their_seals_to_the_performance() {
        let (mut p, impeller, _) = pump_with_volute();
        let base = p.performance_input(impeller).unwrap();
        // Without chambers the gap and the shaft seal stay automatic.
        let plain = p.with_contour_seals(impeller, &base);
        assert!(plain.losses.axial_gap.is_auto());
        assert!(plain.axial.shaft_seal_diameter.is_auto());
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.chambers.enabled = true;
        input.meridional.seals.front.clearance = Param::Manual(0.0004);
        p.set_impeller_geometry(impeller, input).unwrap();
        let v = p
            .compute_impeller_geometry(impeller)
            .unwrap()
            .contour
            .chambers
            .values;
        let shared = p.with_contour_seals(impeller, &base);
        assert_eq!(shared.losses.seal_clearance, Param::Manual(0.0004));
        assert_eq!(shared.axial.seal_diameter, Param::Manual(v.seal_diameter));
        assert_eq!(shared.losses.axial_gap, Param::Manual(v.axial_gap));
        assert_eq!(shared.losses.seal_length, Param::Manual(v.seal_length));
        // A value set in the performance step stays; worn seals stay twice as wide.
        let mut own = base.clone();
        own.losses.axial_gap = Param::Manual(0.01);
        own.wide_seal_clearances = true;
        let kept = p.with_contour_seals(impeller, &own);
        assert_eq!(kept.losses.axial_gap, Param::Manual(0.01));
        assert_eq!(kept.losses.seal_clearance, Param::Manual(0.0008));
        // A wider seal on the neck leaks more: the efficiency drops.
        let tight = {
            let mut q = p.clone();
            let mut input = q.impeller_geometry_input(impeller).unwrap();
            input.meridional.seals.front.clearance = Param::Manual(0.0001);
            q.set_impeller_geometry(impeller, input).unwrap();
            q.compute_performance(impeller, &base).unwrap()
        };
        let loose = p.compute_performance(impeller, &base).unwrap();
        assert!(loose.design.efficiency < tight.design.efficiency);
    }

    /// Seals on, rings off: the automatic seal diameter is d1 + 2t, as the axial thrust
    /// took it before the seals were part of the contour.
    fn plain_seals(p: &mut Project, impeller: ComponentId) {
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        for side in [
            &mut input.meridional.seals.front,
            &mut input.meridional.seals.rear,
        ] {
            side.enabled = true;
            side.impeller_ring.enabled = false;
            side.casing_ring.enabled = false;
        }
        p.set_impeller_geometry(impeller, input).unwrap();
    }

    #[test]
    fn performance_takes_the_front_seal_without_chambers() {
        let (mut p, impeller, _) = pump_with_volute();
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.seals.front.enabled = true;
        input.meridional.seals.front.clearance = Param::Manual(0.0004);
        p.set_impeller_geometry(impeller, input).unwrap();
        let front = p
            .compute_impeller_geometry(impeller)
            .unwrap()
            .contour
            .seals
            .front
            .unwrap();
        let base = p.performance_input(impeller).unwrap();
        let shared = p.with_contour_seals(impeller, &base);
        assert_eq!(
            shared.axial.seal_diameter,
            Param::Manual(2.0 * front.radius)
        );
        assert_eq!(shared.losses.seal_clearance, Param::Manual(0.0004));
        assert_eq!(shared.losses.seal_length, Param::Manual(front.length));
        assert!(shared.losses.axial_gap.is_auto());
        let mut worn = base.clone();
        worn.wide_seal_clearances = true;
        let worn = p.with_contour_seals(impeller, &worn);
        assert_eq!(worn.losses.seal_clearance, Param::Manual(0.0008));
        // The thrust takes the same diameter, the ring included.
        let curve = p.compute_performance(impeller, &base).unwrap();
        assert_eq!(curve.axial.law.seal_diameter, 2.0 * front.radius);
    }

    #[test]
    fn performance_takes_the_rear_seal_for_balance_holes() {
        let (mut p, impeller, _) = pump_with_volute();
        let rear = p
            .compute_impeller_geometry(impeller)
            .unwrap()
            .contour
            .seals
            .rear
            .unwrap();
        let mut base = p.performance_input(impeller).unwrap();
        base.axial.balance_holes = true;
        let shared = p.with_contour_seals(impeller, &base);
        assert_eq!(
            shared.axial.rear_seal_diameter,
            Param::Manual(2.0 * rear.radius)
        );
        let curve = p.compute_performance(impeller, &base).unwrap();
        assert_eq!(curve.axial.law.rear_seal_diameter, Some(2.0 * rear.radius));
        // Without a rear seal the diameter stays automatic.
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.seals.rear.enabled = false;
        p.set_impeller_geometry(impeller, input).unwrap();
        assert!(
            p.with_contour_seals(impeller, &base)
                .axial
                .rear_seal_diameter
                .is_auto()
        );
    }

    #[test]
    fn balance_holes_without_a_rear_seal_are_left_out_with_a_warning() {
        let (mut p, impeller, _) = pump_with_volute();
        let mut base = p.performance_input(impeller).unwrap();
        base.axial.balance_holes = true;
        let with = p.compute_performance(impeller, &base).unwrap();
        assert!(with.axial.law.rear_seal_diameter.is_some());
        assert!(!with.warnings.iter().any(|w| w.contains("balance holes")));
        let mut input = p.impeller_geometry_input(impeller).unwrap();
        input.meridional.seals.rear.enabled = false;
        p.set_impeller_geometry(impeller, input).unwrap();
        let without = p.compute_performance(impeller, &base).unwrap();
        assert!(without.axial.law.rear_seal_diameter.is_none());
        assert!(
            without
                .warnings
                .iter()
                .any(|w| w.contains("balance holes need the rear seal")),
            "{:?}",
            without.warnings
        );
    }

    #[test]
    fn manual_performance_values_beat_the_seals() {
        let (p, impeller, _) = pump_with_volute();
        let mut own = p.performance_input(impeller).unwrap();
        own.axial.seal_diameter = Param::Manual(0.09);
        own.axial.rear_seal_diameter = Param::Manual(0.08);
        own.losses.seal_clearance = Param::Manual(0.0003);
        own.losses.seal_length = Param::Manual(0.02);
        let kept = p.with_contour_seals(impeller, &own);
        assert_eq!(kept, own);
    }

    #[test]
    fn automatic_seals_keep_todays_axial_thrust() {
        let (mut p, impeller, _) = pump_with_volute();
        let base = p.performance_input(impeller).unwrap();
        // Today: the seals off, the thrust takes its own automatic diameter.
        let mut off = p.impeller_geometry_input(impeller).unwrap();
        off.meridional.seals.front.enabled = false;
        off.meridional.seals.rear.enabled = false;
        p.set_impeller_geometry(impeller, off).unwrap();
        let today = p.compute_performance(impeller, &base).unwrap().axial;
        // Seals on without rings: d1 + 2t is the same diameter.
        plain_seals(&mut p, impeller);
        let now = p.compute_performance(impeller, &base).unwrap().axial;
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-300);
        assert!(close(now.law.seal_diameter, today.law.seal_diameter));
        assert!(close(now.design.total, today.design.total));
        assert!(close(now.shutoff.total, today.shutoff.total));
        assert!(close(now.largest, today.largest));
    }

    #[test]
    fn the_default_rings_widen_the_axial_seal_diameter() {
        let (p, impeller, _) = pump_with_volute();
        let base = p.performance_input(impeller).unwrap();
        let front = p
            .compute_impeller_geometry(impeller)
            .unwrap()
            .contour
            .seals
            .front
            .unwrap();
        let axial = p.compute_performance(impeller, &base).unwrap().axial;
        assert_eq!(axial.law.seal_diameter, 2.0 * front.radius);
        let mut plain = p.clone();
        plain_seals(&mut plain, impeller);
        let narrow = plain.compute_performance(impeller, &base).unwrap().axial;
        assert!(axial.law.seal_diameter > narrow.law.seal_diameter);
    }

    #[test]
    fn the_collector_sets_the_performance_prediction() {
        use tp_design::performance::{Collector, ShutoffSource};
        let (mut p, impeller, _) = pump_with_volute();
        assert_eq!(p.collector_after(impeller).unwrap(), Collector::Volute);
        let volute_curve = p
            .compute_performance(impeller, &p.performance_input(impeller).unwrap())
            .unwrap();
        // The automatic volute is nearly the matched one of the loss split; the seals
        // of the loss analysis leak less than the statistics, so the head is a little
        // higher.
        assert_relative_eq!(volute_curve.design.head, 32.0, max_relative = 0.015);
        let data = p.collector_data_after(impeller).unwrap();
        assert_eq!(data.throat.unwrap().count, 1);
        assert!(data.volute.is_some() && volute_curve.volute.is_some());
        assert!(volute_curve.npsh.collector_flow.is_some());

        p.add_component(ComponentKind::Stator { vaned: true }, Some(1))
            .unwrap();
        assert_eq!(
            p.collector_after(impeller).unwrap(),
            Collector::VanedDiffuser
        );
        let diffuser_curve = p
            .compute_performance(impeller, &p.performance_input(impeller).unwrap())
            .unwrap();
        // Diffuser pumps reach a higher head against a closed valve, but in this pump the
        // computed curve lies above both correlations, so both are raised to it.
        assert_eq!(diffuser_curve.shutoff_source, ShutoffSource::Curve);
        assert_eq!(volute_curve.shutoff_source, ShutoffSource::Curve);
        assert_relative_eq!(
            diffuser_curve.shutoff_head,
            volute_curve.shutoff_head,
            max_relative = 0.01
        );
        // The vane passages are the throat; the volute behind them is not modelled.
        let data = p.collector_data_after(impeller).unwrap();
        let vanes = p.compute_diffuser(p.components()[1].id()).unwrap();
        assert_eq!(data.throat.unwrap().count, vanes.vanes.unwrap().count);
        assert!(data.volute.is_none() && diffuser_curve.volute.is_none());
    }

    /// The stage of the design example of Japikse et al. (Chap. 7.5, Fig. 7.73): 5000 gpm,
    /// 84.05 ft and 1000 rpm, with a vaneless diffuser and a volute. Its volute is
    /// matched at the design point (λ·AR = 1.0) with λ = 4.18 and a spiral loss
    /// coefficient of 0.054.
    #[test]
    fn the_volute_of_the_book_example_stage_is_matched_like_ours() {
        use crate::{DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType};
        let gpm = 3.785_411_784e-3 / 60.0;
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(5000.0 * gpm),
                energy: EnergyTransfer::Head(84.05 * 0.3048),
                speed: 1000.0 * std::f64::consts::PI / 30.0,
            },
        );
        let mut p = Project::new("Book example", MachineType::Pump, setup).unwrap();
        let impeller = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Stator { vaned: false }, None)
            .unwrap();
        p.add_component(ComponentKind::Volute, None).unwrap();
        let curve = p
            .compute_performance(impeller, &p.performance_input(impeller).unwrap())
            .unwrap();
        let v = curve.volute.unwrap();
        assert_relative_eq!(v.design.swirl, 4.18, max_relative = 0.05);
        assert_relative_eq!(v.design.matching, 1.0, epsilon = 0.05);
        assert_relative_eq!(v.design.loss_coefficient, 0.054, epsilon = 0.003);
    }

    #[test]
    fn a_volute_behind_a_vaneless_diffuser_still_counts() {
        let (mut p, impeller, _) = pump_with_volute();
        p.add_component(ComponentKind::Stator { vaned: false }, Some(1))
            .unwrap();
        let data = p.collector_data_after(impeller).unwrap();
        let volute = data.volute.unwrap();
        // The spiral starts beyond the diffuser outlet.
        let diffuser = p.compute_diffuser(p.components()[1].id()).unwrap();
        assert!(volute.inlet_radius > diffuser.outlet_diameter / 2.0);
        let curve = p
            .compute_performance(impeller, &p.performance_input(impeller).unwrap())
            .unwrap();
        assert!(curve.volute.is_some());
    }

    #[test]
    fn volute_follows_the_impeller_outlet() {
        let (p, impeller, volute) = pump_with_volute();
        let basis = p.volute_basis(volute).unwrap();
        let main = p.compute_main_dimensions(impeller).unwrap();
        let g = p.compute_impeller_geometry(impeller).unwrap();
        assert_relative_eq!(basis.impeller_diameter, main.outlet_diameter);
        assert_relative_eq!(basis.cu, main.outlet.triangle.cu);
        assert_relative_eq!(basis.flow, 100.0 / 3600.0, max_relative = 1e-12);
        assert_relative_eq!(
            basis.midplane,
            g.contour.axial_length - main.outlet_width / 2.0,
            max_relative = 1e-12
        );
        let v = p.compute_volute(volute).unwrap();
        assert!(v.design.cutwater_diameter > main.outlet_diameter);
        // The faces of the impeller beside it, with the walls of its CAD model: the hub
        // disk flat behind the outlet, the shroud in front coming back to the eye.
        let sides = basis.sides.unwrap();
        let wall = tp_design::impeller::default_wall_thickness(main.outlet_diameter);
        let length = g.contour.axial_length;
        for q in sides.plus {
            assert_relative_eq!(q.y, length + wall, epsilon = 1e-12);
        }
        let r2 = main.outlet_diameter / 2.0;
        assert_relative_eq!(sides.minus[0].x, r2, max_relative = 1e-4);
        assert_relative_eq!(
            sides.minus[0].y,
            length - main.outlet_width - wall,
            epsilon = 1e-5
        );
        let eye = sides.minus.last().unwrap();
        assert!(eye.y.abs() < 1e-4, "{eye:?}");
        assert!(eye.x > main.inlet_diameter / 2.0, "{eye:?}");
        for w in sides.minus.windows(2) {
            assert!(w[1].x < w[0].x && w[1].y <= w[0].y + 1e-12, "{w:?}");
        }
    }

    #[test]
    fn volute_steps_and_downstream_staleness() {
        let (mut p, impeller, volute) = pump_with_volute();
        for step in VOLUTE_STEPS {
            p.design_step(volute, step).unwrap();
        }
        assert_eq!(
            p.component(volute).unwrap().next_step(),
            Some(DesignStep::CfdSetup)
        );
        assert!(matches!(
            p.design_step(volute, DesignStep::CfdSetup),
            Err(ProjectError::Unsupported(_))
        ));

        // Blade edges do not move the outlet; the main dimensions do.
        let mut geometry = p.impeller_geometry_input(impeller).unwrap();
        geometry.edges.trailing = tp_design::impeller::blade::EdgeShape::Ellipse { ratio: 2.0 };
        p.set_impeller_geometry(impeller, geometry).unwrap();
        assert_eq!(
            p.component(volute).unwrap().status(DesignStep::VoluteSetup),
            StepStatus::Done
        );
        let input = CentrifugalPumpInput {
            blade_count: Param::Manual(6),
            ..Default::default()
        };
        p.set_main_dimensions(impeller, MainDimensionsInput::CentrifugalPump(input))
            .unwrap();
        assert_eq!(
            p.component(volute).unwrap().status(DesignStep::VoluteSetup),
            StepStatus::Stale
        );
    }

    #[test]
    fn volute_inputs_invalidate_from_their_step_and_persist() {
        let (mut p, _, volute) = pump_with_volute();
        for step in VOLUTE_STEPS {
            p.design_step(volute, step).unwrap();
        }
        let mut input = p.volute_input(volute).unwrap();
        input.diffuser.length = Param::Manual(0.2);
        p.set_volute(volute, input.clone()).unwrap();
        let c = p.component(volute).unwrap();
        assert_eq!(c.status(DesignStep::SpiralDevelopment), StepStatus::Done);
        assert_eq!(c.status(DesignStep::Diffuser), StepStatus::Stale);

        let loaded = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(loaded.volute_input(volute).unwrap(), input);
    }

    #[test]
    fn volutes_need_an_impeller_upstream() {
        let mut p = pump();
        let inlet = p.add_component(ComponentKind::Volute, None).unwrap();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        assert!(matches!(
            p.compute_volute(inlet),
            Err(ProjectError::Unsupported(_))
        ));
        let (p, impeller, _) = pump_with_volute();
        assert!(matches!(
            p.volute_input(impeller),
            Err(ProjectError::NotAVolute(_))
        ));
    }

    #[test]
    fn multistage_pump_with_diffuser_and_volute() {
        let mut p = pump();
        let first = p.add_component(ComponentKind::Impeller, None).unwrap();
        let diffuser = p
            .add_component(ComponentKind::Stator { vaned: true }, None)
            .unwrap();
        let second = p.add_component(ComponentKind::Impeller, None).unwrap();
        let volute = p.add_component(ComponentKind::Volute, None).unwrap();

        // Each stage takes half the head; the diffuser sees the first impeller.
        let d = p.compute_diffuser(diffuser).unwrap();
        let main = p.compute_main_dimensions(first).unwrap();
        assert!(d.inlet_diameter > main.outlet_diameter);
        assert!(d.vanes.is_some());
        assert_relative_eq!(p.pump_duty(second).unwrap().head, 16.0);
        p.compute_volute(volute).unwrap();

        for step in VANED_STEPS {
            p.design_step(diffuser, step).unwrap();
        }
        assert_eq!(
            p.component(diffuser).unwrap().next_step(),
            Some(DesignStep::CfdSetup)
        );
        // A new first impeller makes everything after it stale.
        let input = CentrifugalPumpInput {
            blade_count: Param::Manual(6),
            ..Default::default()
        };
        p.set_main_dimensions(first, MainDimensionsInput::CentrifugalPump(input))
            .unwrap();
        assert_eq!(
            p.component(diffuser)
                .unwrap()
                .status(DesignStep::MainDimensions),
            StepStatus::Stale
        );
    }

    #[test]
    fn volute_after_a_diffuser_takes_its_outlet() {
        let mut p = pump();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        let diffuser = p
            .add_component(ComponentKind::Stator { vaned: true }, None)
            .unwrap();
        let volute = p.add_component(ComponentKind::Volute, None).unwrap();
        let d = p.compute_diffuser(diffuser).unwrap();
        let basis = p.volute_basis(volute).unwrap();
        assert_relative_eq!(basis.impeller_diameter, d.outlet_diameter);
        assert_relative_eq!(basis.cu, d.outlet.cu);
        // The diffuser walls, not the impeller, stand beside the volute.
        assert!(basis.sides.is_none());
        let v = p.compute_volute(volute).unwrap();
        assert!(v.design.cutwater_diameter > d.outlet_diameter);
    }

    #[test]
    fn diffuser_inputs_invalidate_from_their_step() {
        let mut p = pump();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        let ring = p
            .add_component(ComponentKind::Stator { vaned: false }, None)
            .unwrap();
        for step in VANELESS_STEPS {
            p.design_step(ring, step).unwrap();
        }
        let mut input = p.diffuser_input(ring).unwrap();
        input.contour.outlet_width = Param::Manual(0.02);
        p.set_diffuser(ring, input.clone()).unwrap();
        let c = p.component(ring).unwrap();
        assert_eq!(c.status(DesignStep::MainDimensions), StepStatus::Done);
        assert_eq!(c.status(DesignStep::MeridionalContour), StepStatus::Stale);
        let loaded = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(loaded.diffuser_input(ring).unwrap(), input);
        assert!(matches!(
            p.design_step(ring, DesignStep::BladeProperties),
            Err(ProjectError::StepNotApplicable { .. } | ProjectError::Unsupported(_))
        ));

        let mut q = pump();
        let lone = q
            .add_component(ComponentKind::Stator { vaned: true }, None)
            .unwrap();
        assert!(matches!(
            q.compute_diffuser(lone),
            Err(ProjectError::Unsupported(_))
        ));
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

    #[test]
    fn a_double_volute_cuts_the_radial_force_in_the_prediction() {
        use tp_design::volute::VoluteKind;
        let (mut p, impeller, volute) = pump_with_volute();
        let input = p.performance_input(impeller).unwrap();
        let single = p.compute_performance(impeller, &input).unwrap();
        let mut v = p.volute_input(volute).unwrap();
        v.setup.kind = VoluteKind::Double;
        p.set_volute(volute, v).unwrap();
        let data = p.collector_data_after(impeller).unwrap();
        assert_eq!(data.throat.unwrap().count, 2);
        assert_relative_eq!(data.double_volute_wrap.unwrap(), std::f64::consts::PI);
        let double = p.compute_performance(impeller, &input).unwrap();
        let (s, d) = (single.radial.unwrap(), double.radial.unwrap());
        assert!(d.shutoff_force < s.shutoff_force / 3.0);
        // Two partial volutes and the outer channel rub on more wall (Gülich Chap. 3.8):
        // a little less head.
        assert!(double.design.head < single.design.head);
        assert_relative_eq!(double.design.head, single.design.head, max_relative = 0.03);
    }
}
