use serde::{Deserialize, Serialize};
use tp_design::acceptance::AcceptanceInput;
use tp_units::DisplayUnits;

use crate::{
    Component, ComponentId, ComponentKind, GlobalSetup, MachineType, ProjectError, StepStatus,
};

/// Descriptive information about a project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
}

/// How serious an [`Issue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// A problem found when checking a project.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
    pub component: Option<ComponentId>,
}

/// A complete machine: global setup plus an ordered list of components, from inlet to
/// outlet.
///
/// Composition rules: at most [`MachineType::max_impellers`] impellers, any number of
/// stators, and volutes only as the first or last component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub info: ProjectInfo,
    machine: MachineType,
    setup: GlobalSetup,
    #[serde(default)]
    pub display_units: DisplayUnits,
    /// How the predicted performance is judged against the design point. Not a design
    /// input: changing it makes no step stale.
    #[serde(default)]
    pub acceptance: AcceptanceInput,
    components: Vec<Component>,
    next_id: u32,
}

impl Project {
    /// Creates an empty project after validating `setup` for `machine`.
    pub fn new(
        name: impl Into<String>,
        machine: MachineType,
        setup: GlobalSetup,
    ) -> Result<Self, ProjectError> {
        setup.validate(machine)?;
        Ok(Self {
            info: ProjectInfo {
                name: name.into(),
                ..ProjectInfo::default()
            },
            machine,
            setup,
            display_units: DisplayUnits::default(),
            acceptance: AcceptanceInput::default(),
            components: Vec::new(),
            next_id: 1,
        })
    }

    pub fn machine(&self) -> MachineType {
        self.machine
    }

    pub fn setup(&self) -> &GlobalSetup {
        &self.setup
    }

    /// Replaces the global setup. If it changed, every component's design becomes stale,
    /// since all of them derive from the design point.
    pub fn set_setup(&mut self, setup: GlobalSetup) -> Result<(), ProjectError> {
        setup.validate(self.machine)?;
        if setup != self.setup {
            self.setup = setup;
            for c in &mut self.components {
                c.invalidate_all();
            }
        }
        Ok(())
    }

    /// Components from inlet to outlet.
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    pub fn component(&self, id: ComponentId) -> Option<&Component> {
        self.components.iter().find(|c| c.id() == id)
    }

    pub fn component_mut(&mut self, id: ComponentId) -> Result<&mut Component, ProjectError> {
        self.components
            .iter_mut()
            .find(|c| c.id() == id)
            .ok_or(ProjectError::UnknownComponent(id))
    }

    /// Inserts a new component at `position` (default: after the last one).
    pub fn add_component(
        &mut self,
        kind: ComponentKind,
        position: Option<usize>,
    ) -> Result<ComponentId, ProjectError> {
        let len = self.components.len();
        let position = position.unwrap_or(len);
        if position > len {
            return Err(ProjectError::InvalidPosition { position, len });
        }
        let id = ComponentId(self.next_id);
        let same_kind = self
            .components
            .iter()
            .filter(|c| c.kind().label() == kind.label())
            .count();
        let name = format!("{} {}", kind.label(), same_kind + 1);
        self.components
            .insert(position, Component::new(id, name, kind));
        if let Err(e) = self.check_composition() {
            self.components.remove(position);
            return Err(e);
        }
        self.next_id += 1;
        self.invalidate_neighbours(position);
        Ok(id)
    }

    /// Removes a component. Its neighbours are invalidated, since their coupled
    /// boundaries change.
    pub fn remove_component(&mut self, id: ComponentId) -> Result<Component, ProjectError> {
        let index = self
            .components
            .iter()
            .position(|c| c.id() == id)
            .ok_or(ProjectError::UnknownComponent(id))?;
        let removed = self.components.remove(index);
        if let Err(e) = self.check_composition() {
            self.components.insert(index, removed);
            return Err(e);
        }
        // After removal the former neighbours sit at index - 1 and index.
        if index > 0 {
            self.components[index - 1].invalidate_all();
        }
        if let Some(next) = self.components.get_mut(index) {
            next.invalidate_all();
        }
        Ok(removed)
    }

    fn invalidate_neighbours(&mut self, index: usize) {
        if index > 0 {
            self.components[index - 1].invalidate_all();
        }
        if let Some(next) = self.components.get_mut(index + 1) {
            next.invalidate_all();
        }
    }

    pub(crate) fn check_composition(&self) -> Result<(), ProjectError> {
        let impellers = self
            .components
            .iter()
            .filter(|c| c.kind() == ComponentKind::Impeller)
            .count();
        let max = self.machine.max_impellers();
        if impellers > max {
            return Err(ProjectError::TooManyImpellers { max });
        }
        let last = self.components.len().saturating_sub(1);
        let volute_inside = self
            .components
            .iter()
            .enumerate()
            .any(|(i, c)| c.kind() == ComponentKind::Volute && i != 0 && i != last);
        if volute_inside {
            return Err(ProjectError::VoluteNotAtEnd);
        }
        let kind = |i: usize| self.components.get(i).map(|c| c.kind());
        let duct_misplaced = self.components.iter().enumerate().any(|(i, c)| {
            c.kind() == ComponentKind::InletDuct
                && (i != 0 || kind(1) != Some(ComponentKind::Impeller))
        });
        if duct_misplaced {
            return Err(ProjectError::DuctNotBeforeImpeller);
        }
        Ok(())
    }

    /// Restores invariants that a hand-edited file could break.
    pub(crate) fn repair_after_load(&mut self) -> Result<(), ProjectError> {
        self.setup.validate(self.machine)?;
        let max_id = self.components.iter().map(|c| c.id().0).max().unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
        self.move_legacy_duct()?;
        self.check_composition()?;
        Ok(())
    }

    /// Older files keep the inlet duct inside the meridional contour of the impeller,
    /// with a switch: a modelled one becomes a component of its own ahead of the first
    /// impeller, and the switch goes.
    fn move_legacy_duct(&mut self) -> Result<(), ProjectError> {
        let mut found = None;
        for (i, c) in self.components.iter_mut().enumerate() {
            let Some(mut geometry) = c.geometry().cloned() else {
                continue;
            };
            let legacy = std::mem::take(&mut geometry.meridional.inlet_duct);
            if legacy != Default::default() {
                // Clearing what is only read changes no step.
                c.set_geometry_unchecked(geometry);
            }
            if let Some(input) = legacy.input()
                && i == 0
            {
                found = Some(input);
            }
        }
        let first_is_duct =
            self.components.first().map(|c| c.kind()) == Some(ComponentKind::InletDuct);
        if let Some(input) = found
            && !first_is_duct
        {
            let id = ComponentId(self.next_id);
            self.next_id += 1;
            let mut duct = Component::new(id, "Inlet duct 1".into(), ComponentKind::InletDuct);
            duct.set_duct(input)?;
            self.components.insert(0, duct);
        }
        Ok(())
    }

    /// Everything that keeps the project from being a complete, consistent design.
    pub fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        if let Err(e) = self.setup.validate(self.machine) {
            issues.push(Issue {
                severity: Severity::Error,
                message: e.to_string(),
                component: None,
            });
        }
        if !self
            .components
            .iter()
            .any(|c| c.kind() == ComponentKind::Impeller)
        {
            issues.push(Issue {
                severity: Severity::Warning,
                message: "the project has no impeller".into(),
                component: None,
            });
        }
        for c in &self.components {
            let stale: Vec<String> = c
                .steps()
                .filter(|&(_, s)| s == StepStatus::Stale)
                .map(|(step, _)| step.to_string())
                .collect();
            if !stale.is_empty() {
                issues.push(Issue {
                    severity: Severity::Warning,
                    message: format!("{}: outdated steps: {}", c.name, stale.join(", ")),
                    component: Some(c.id()),
                });
            }
        }
        issues
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{DesignPoint, DesignStep, EnergyTransfer, FlowRate};

    pub(crate) fn pump() -> Project {
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(100.0 / 3600.0),
                energy: EnergyTransfer::Head(32.0),
                speed: 2900.0 * std::f64::consts::PI / 30.0,
            },
        );
        Project::new("Test pump", MachineType::Pump, setup).unwrap()
    }

    #[test]
    fn names_and_ids_are_assigned() {
        let mut p = pump();
        let a = p.add_component(ComponentKind::Impeller, None).unwrap();
        let b = p.add_component(ComponentKind::Impeller, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(p.component(b).unwrap().name, "Impeller 2");
    }

    #[test]
    fn volutes_only_at_the_ends() {
        let mut p = pump();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Volute, None).unwrap();
        p.add_component(ComponentKind::Volute, Some(0)).unwrap();
        assert!(matches!(
            p.add_component(ComponentKind::Volute, Some(1)),
            Err(ProjectError::VoluteNotAtEnd)
        ));
        // Appending after the outlet volute would leave it inside the machine.
        assert!(matches!(
            p.add_component(ComponentKind::Stator { vaned: true }, None),
            Err(ProjectError::VoluteNotAtEnd)
        ));
        assert_eq!(p.components().len(), 3);
    }

    #[test]
    fn impeller_limit_depends_on_machine() {
        let mut p = pump();
        for _ in 0..5 {
            p.add_component(ComponentKind::Impeller, None).unwrap();
        }
        assert!(matches!(
            p.add_component(ComponentKind::Impeller, None),
            Err(ProjectError::TooManyImpellers { max: 5 })
        ));
        assert_eq!(MachineType::HydroTurbine.max_impellers(), 1);
    }

    #[test]
    fn setup_change_invalidates_components() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.component_mut(id)
            .unwrap()
            .complete(DesignStep::MainDimensions)
            .unwrap();

        let unchanged = p.setup().clone();
        p.set_setup(unchanged).unwrap();
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::MainDimensions),
            StepStatus::Done
        );

        let mut changed = p.setup().clone();
        changed.design_point.speed *= 1.1;
        p.set_setup(changed).unwrap();
        assert_eq!(
            p.component(id).unwrap().status(DesignStep::MainDimensions),
            StepStatus::Stale
        );
        assert_eq!(p.issues().len(), 1);
    }

    #[test]
    fn invalid_setup_is_rejected_and_kept() {
        let mut p = pump();
        let before = p.setup().clone();
        let mut bad = before.clone();
        bad.design_point.speed = -1.0;
        assert!(p.set_setup(bad).is_err());
        assert_eq!(p.setup(), &before);
    }

    #[test]
    fn inserting_and_removing_invalidates_neighbours() {
        let mut p = pump();
        let imp = p.add_component(ComponentKind::Impeller, None).unwrap();
        let vol = p.add_component(ComponentKind::Volute, None).unwrap();
        for id in [imp, vol] {
            let c = p.component_mut(id).unwrap();
            let first = c.kind().steps()[0];
            c.complete(first).unwrap();
        }
        let stator = p
            .add_component(ComponentKind::Stator { vaned: true }, Some(1))
            .unwrap();
        assert!(
            p.components()[0]
                .steps()
                .any(|(_, s)| s == StepStatus::Stale)
        );
        assert!(
            p.components()[2]
                .steps()
                .any(|(_, s)| s == StepStatus::Stale)
        );

        p.remove_component(stator).unwrap();
        assert!(matches!(
            p.remove_component(stator),
            Err(ProjectError::UnknownComponent(_))
        ));
    }

    #[test]
    fn empty_project_warns_about_missing_impeller() {
        let issues = pump().issues();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }
}
