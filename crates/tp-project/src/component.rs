use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tp_design::MainDimensionsInput;
use tp_design::impeller::ImpellerGeometryInput;

use crate::ProjectError;

/// Stable identifier of a component within its project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub u32);

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What a component is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComponentKind {
    /// Rotating bladed component (runner, for turbines).
    Impeller,
    /// Stationary component, with or without vanes.
    Stator { vaned: bool },
    /// Spiral casing at the inlet or outlet of the machine.
    Volute,
}

impl ComponentKind {
    /// The design steps of this kind of component, in the order they are completed.
    pub fn steps(self) -> &'static [DesignStep] {
        use DesignStep::*;
        match self {
            ComponentKind::Impeller | ComponentKind::Stator { vaned: true } => &[
                MainDimensions,
                MeridionalContour,
                BladeProperties,
                BladeMeanLines,
                BladeProfiles,
                BladeEdges,
                CfdSetup,
                ModelSettings,
                ModelFinishing,
            ],
            ComponentKind::Stator { vaned: false } => {
                &[MainDimensions, MeridionalContour, CfdSetup, ModelSettings]
            }
            ComponentKind::Volute => &[
                VoluteSetup,
                CrossSection,
                SpiralDevelopment,
                Diffuser,
                CutWater,
                WallThickness,
                CfdSetup,
                ModelSettings,
            ],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ComponentKind::Impeller => "Impeller",
            ComponentKind::Stator { vaned: true } => "Stator",
            ComponentKind::Stator { vaned: false } => "Vaneless stator",
            ComponentKind::Volute => "Volute",
        }
    }
}

/// One stage of a component's design. Later steps build on earlier ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignStep {
    MainDimensions,
    MeridionalContour,
    BladeProperties,
    BladeMeanLines,
    BladeProfiles,
    BladeEdges,
    VoluteSetup,
    CrossSection,
    SpiralDevelopment,
    Diffuser,
    CutWater,
    WallThickness,
    CfdSetup,
    ModelSettings,
    ModelFinishing,
}

impl fmt::Display for DesignStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DesignStep::MainDimensions => "main dimensions",
            DesignStep::MeridionalContour => "meridional contour",
            DesignStep::BladeProperties => "blade properties",
            DesignStep::BladeMeanLines => "blade mean lines",
            DesignStep::BladeProfiles => "blade profiles",
            DesignStep::BladeEdges => "blade edges",
            DesignStep::VoluteSetup => "volute setup",
            DesignStep::CrossSection => "cross section",
            DesignStep::SpiralDevelopment => "spiral development",
            DesignStep::Diffuser => "diffuser",
            DesignStep::CutWater => "cut-water",
            DesignStep::WallThickness => "wall thickness",
            DesignStep::CfdSetup => "CFD setup",
            DesignStep::ModelSettings => "model settings",
            DesignStep::ModelFinishing => "model finishing",
        })
    }
}

/// Progress of one design step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Never completed.
    #[default]
    Pending,
    /// Completed and consistent with its inputs.
    Done,
    /// Completed, but an input changed since; it must be reviewed or recomputed.
    Stale,
}

/// How a component's boundary is tied to its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coupling {
    /// The inlet follows the outlet of the previous component.
    #[default]
    FlowDirection,
    /// The outlet follows the inlet of the next component.
    ReverseFlowDirection,
    /// Inlet and outlet are independent; gaps between components are allowed.
    Uncoupled,
}

/// An impeller, stator or volute, with the progress of its design steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    id: ComponentId,
    pub name: String,
    kind: ComponentKind,
    pub coupling: Coupling,
    /// Steps not listed are pending.
    #[serde(default)]
    steps: BTreeMap<DesignStep, StepStatus>,
    /// Inputs of the main-dimensions step; `None` means every value automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    main_dimensions: Option<MainDimensionsInput>,
    /// Inputs of the geometry steps, from meridional contour to blade edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    geometry: Option<ImpellerGeometryInput>,
}

impl Component {
    pub(crate) fn new(id: ComponentId, name: String, kind: ComponentKind) -> Self {
        Self {
            id,
            name,
            kind,
            coupling: Coupling::default(),
            steps: BTreeMap::new(),
            main_dimensions: None,
            geometry: None,
        }
    }

    /// Inputs of the geometry steps, if any were set.
    pub fn geometry(&self) -> Option<&ImpellerGeometryInput> {
        self.geometry.as_ref()
    }

    /// Replaces the geometry inputs. Each step whose inputs changed, and every step
    /// after it, becomes stale.
    pub fn set_geometry(&mut self, input: ImpellerGeometryInput) -> Result<(), ProjectError> {
        if self.kind != ComponentKind::Impeller {
            return Err(ProjectError::NotAnImpeller(self.id));
        }
        let old = self.geometry.clone().unwrap_or_default();
        let first_changed = [
            (
                old.meridional != input.meridional,
                DesignStep::MeridionalContour,
            ),
            (
                old.properties != input.properties,
                DesignStep::BladeProperties,
            ),
            (
                old.mean_lines != input.mean_lines,
                DesignStep::BladeMeanLines,
            ),
            (old.profiles != input.profiles, DesignStep::BladeProfiles),
            (old.edges != input.edges, DesignStep::BladeEdges),
        ]
        .into_iter()
        .find(|(changed, _)| *changed)
        .map(|(_, step)| step);
        self.geometry = Some(input);
        if let Some(step) = first_changed {
            self.invalidate_from(step)?;
        }
        Ok(())
    }

    /// Inputs of the main-dimensions step, if any were set.
    pub fn main_dimensions(&self) -> Option<&MainDimensionsInput> {
        self.main_dimensions.as_ref()
    }

    /// Replaces the main-dimensions inputs. A change makes the whole design stale,
    /// since every later step builds on the main dimensions.
    pub fn set_main_dimensions(&mut self, input: MainDimensionsInput) -> Result<(), ProjectError> {
        if self.kind != ComponentKind::Impeller {
            return Err(ProjectError::NotAnImpeller(self.id));
        }
        if self.main_dimensions.as_ref() != Some(&input) {
            self.main_dimensions = Some(input);
            self.invalidate_all();
        }
        Ok(())
    }

    pub fn id(&self) -> ComponentId {
        self.id
    }

    pub fn kind(&self) -> ComponentKind {
        self.kind
    }

    /// Every design step of this component with its status, in design order.
    pub fn steps(&self) -> impl Iterator<Item = (DesignStep, StepStatus)> + '_ {
        self.kind.steps().iter().map(|&s| (s, self.status(s)))
    }

    pub fn status(&self, step: DesignStep) -> StepStatus {
        self.steps.get(&step).copied().unwrap_or_default()
    }

    /// The first step that is not done, or `None` when the design is complete.
    pub fn next_step(&self) -> Option<DesignStep> {
        self.steps()
            .find(|&(_, status)| status != StepStatus::Done)
            .map(|(step, _)| step)
    }

    pub fn is_complete(&self) -> bool {
        self.next_step().is_none()
    }

    fn position(&self, step: DesignStep) -> Result<usize, ProjectError> {
        self.kind
            .steps()
            .iter()
            .position(|&s| s == step)
            .ok_or(ProjectError::StepNotApplicable { step })
    }

    /// Marks `step` as done. Every earlier step must already be done.
    pub fn complete(&mut self, step: DesignStep) -> Result<(), ProjectError> {
        let index = self.position(step)?;
        if let Some(&previous) = self.kind.steps()[..index]
            .iter()
            .find(|&&s| self.status(s) != StepStatus::Done)
        {
            return Err(ProjectError::PreviousStepIncomplete { step, previous });
        }
        self.steps.insert(step, StepStatus::Done);
        Ok(())
    }

    /// Marks `step` and every later step that was done as stale.
    pub fn invalidate_from(&mut self, step: DesignStep) -> Result<(), ProjectError> {
        let index = self.position(step)?;
        for &s in &self.kind.steps()[index..] {
            if let Some(status) = self.steps.get_mut(&s)
                && *status == StepStatus::Done
            {
                *status = StepStatus::Stale;
            }
        }
        Ok(())
    }

    /// Marks every completed step as stale.
    pub fn invalidate_all(&mut self) {
        let first = self.kind.steps()[0];
        self.invalidate_from(first)
            .expect("the first step belongs to the component");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DesignStep::*;

    fn impeller() -> Component {
        Component::new(ComponentId(1), "Impeller 1".into(), ComponentKind::Impeller)
    }

    #[test]
    fn steps_complete_in_order() {
        let mut c = impeller();
        assert_eq!(c.next_step(), Some(MainDimensions));
        assert!(matches!(
            c.complete(MeridionalContour),
            Err(ProjectError::PreviousStepIncomplete {
                previous: MainDimensions,
                ..
            })
        ));
        c.complete(MainDimensions).unwrap();
        c.complete(MeridionalContour).unwrap();
        assert_eq!(c.next_step(), Some(BladeProperties));
    }

    #[test]
    fn invalidation_marks_downstream_steps_stale() {
        let mut c = impeller();
        for step in [MainDimensions, MeridionalContour, BladeProperties] {
            c.complete(step).unwrap();
        }
        c.invalidate_from(MeridionalContour).unwrap();
        assert_eq!(c.status(MainDimensions), StepStatus::Done);
        assert_eq!(c.status(MeridionalContour), StepStatus::Stale);
        assert_eq!(c.status(BladeProperties), StepStatus::Stale);
        assert_eq!(c.status(BladeMeanLines), StepStatus::Pending);
        assert_eq!(c.next_step(), Some(MeridionalContour));

        // Redoing a stale step requires its predecessors to be done, not its successors.
        c.complete(MeridionalContour).unwrap();
        assert_eq!(c.status(BladeProperties), StepStatus::Stale);
    }

    #[test]
    fn rejects_steps_of_other_component_kinds() {
        let mut c = impeller();
        assert!(matches!(
            c.complete(CutWater),
            Err(ProjectError::StepNotApplicable { .. })
        ));
        let vaneless = ComponentKind::Stator { vaned: false };
        assert!(!vaneless.steps().contains(&BladeProperties));
    }

    #[test]
    fn complete_component() {
        let mut c = Component::new(
            ComponentId(2),
            "Diffuser".into(),
            ComponentKind::Stator { vaned: false },
        );
        for &step in (ComponentKind::Stator { vaned: false }).steps() {
            c.complete(step).unwrap();
        }
        assert!(c.is_complete());
        c.invalidate_all();
        assert!(c.steps().all(|(_, s)| s == StepStatus::Stale));
    }
}
