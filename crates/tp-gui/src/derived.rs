//! Results computed from the project, recomputed only when the project changes. While a
//! drag edits only a volute, only that volute is recomputed; the rest follows when the
//! drag ends.

use std::collections::BTreeMap;

use tp_design::diffuser::DiffuserDesign;
use tp_design::impeller::ImpellerGeometry;
use tp_design::performance::PumpPerformance;
use tp_design::pump::CentrifugalPumpDesign;
use tp_design::volute::VoluteGeometry;
use tp_fluids::LiquidProperties;
use tp_project::{ComponentId, ComponentKind, Project};

/// Design results of one impeller.
#[derive(Debug)]
pub struct ImpellerResults {
    pub main: Result<CentrifugalPumpDesign, String>,
    pub geometry: Result<ImpellerGeometry, String>,
    /// Predicted characteristic with the automatic settings.
    pub performance: Result<PumpPerformance, String>,
}

/// Everything the views show that is computed from the project.
#[derive(Debug, Default)]
pub struct Derived {
    /// The project these results belong to.
    source: Option<Project>,
    /// Increases every time the results are recomputed.
    revision: u64,
    pub liquid: Option<Result<LiquidProperties, String>>,
    pub impellers: BTreeMap<ComponentId, ImpellerResults>,
    pub volutes: BTreeMap<ComponentId, Result<VoluteGeometry, String>>,
    pub diffusers: BTreeMap<ComponentId, Result<DiffuserDesign, String>>,
    /// Only a volute was recomputed during a drag: the other results may lag.
    partial: bool,
}

/// The one volute whose inputs alone tell `old` from `new`, if that is all that changed.
fn only_volute_changed(old: &Project, new: &Project) -> Option<ComponentId> {
    if old.components().len() != new.components().len() {
        return None;
    }
    let mut changed = old
        .components()
        .iter()
        .zip(new.components())
        .filter(|(a, b)| a != b);
    let (a, b) = changed.next()?;
    if changed.next().is_some() || a.id() != b.id() || a.kind() != ComponentKind::Volute {
        return None;
    }
    // Nothing else in the project may differ.
    let mut probe = old.clone();
    *probe.component_mut(a.id()).ok()? = b.clone();
    (probe == *new).then_some(a.id())
}

impl Derived {
    /// Recomputes the results if `project` differs from the one they were computed for.
    /// Returns whether anything was recomputed.
    #[cfg(test)]
    pub fn update(&mut self, project: &Project) -> bool {
        self.update_while(project, false)
    }

    /// As [`Derived::update`], but while `dragging` an edit of one volute recomputes only
    /// that volute; once the drag ends everything is brought up to date.
    pub fn update_while(&mut self, project: &Project, dragging: bool) -> bool {
        if self.source.as_ref() == Some(project) {
            if self.partial && !dragging {
                self.recompute(project);
                return true;
            }
            return false;
        }
        if dragging
            && let Some(old) = &self.source
            && let Some(id) = only_volute_changed(old, project)
        {
            self.volutes
                .insert(id, project.compute_volute(id).map_err(|e| e.to_string()));
            self.source = Some(project.clone());
            self.revision += 1;
            self.partial = true;
            return true;
        }
        self.recompute(project);
        true
    }

    fn recompute(&mut self, project: &Project) {
        self.liquid = Some(project.liquid().map_err(|e| e.to_string()));
        self.impellers = project
            .components()
            .iter()
            .filter(|c| c.kind() == ComponentKind::Impeller)
            .map(|c| {
                let id = c.id();
                let main = project
                    .compute_main_dimensions(id)
                    .map_err(|e| e.to_string());
                let geometry = match &main {
                    Ok(_) => project
                        .compute_impeller_geometry(id)
                        .map_err(|e| e.to_string()),
                    Err(_) => Err("the main dimensions could not be computed".into()),
                };
                let performance = match &main {
                    Ok(_) => project
                        .performance_input(id)
                        .and_then(|input| project.compute_performance(id, &input))
                        .map_err(|e| e.to_string()),
                    Err(_) => Err("the main dimensions could not be computed".into()),
                };
                (
                    id,
                    ImpellerResults {
                        main,
                        geometry,
                        performance,
                    },
                )
            })
            .collect();
        self.volutes = project
            .components()
            .iter()
            .filter(|c| c.kind() == ComponentKind::Volute)
            .map(|c| {
                (
                    c.id(),
                    project.compute_volute(c.id()).map_err(|e| e.to_string()),
                )
            })
            .collect();
        self.diffusers = project
            .components()
            .iter()
            .filter(|c| matches!(c.kind(), ComponentKind::Stator { .. }))
            .map(|c| {
                let result = project.compute_diffuser(c.id()).map_err(|e| e.to_string());
                (c.id(), result)
            })
            .collect();
        self.source = Some(project.clone());
        self.revision += 1;
        self.partial = false;
    }

    /// Changes whenever the results change; views use it to rebuild cached data.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn impeller(&self, id: ComponentId) -> Option<&ImpellerResults> {
        self.impellers.get(&id)
    }

    pub fn main(&self, id: ComponentId) -> Option<&CentrifugalPumpDesign> {
        self.impeller(id).and_then(|r| r.main.as_ref().ok())
    }

    pub fn geometry(&self, id: ComponentId) -> Option<&ImpellerGeometry> {
        self.impeller(id).and_then(|r| r.geometry.as_ref().ok())
    }

    pub fn volute_result(&self, id: ComponentId) -> Option<&Result<VoluteGeometry, String>> {
        self.volutes.get(&id)
    }

    pub fn volute(&self, id: ComponentId) -> Option<&VoluteGeometry> {
        self.volute_result(id).and_then(|r| r.as_ref().ok())
    }

    pub fn diffuser_result(&self, id: ComponentId) -> Option<&Result<DiffuserDesign, String>> {
        self.diffusers.get(&id)
    }

    pub fn diffuser(&self, id: ComponentId) -> Option<&DiffuserDesign> {
        self.diffuser_result(id).and_then(|r| r.as_ref().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_project;

    #[test]
    fn recomputes_only_after_changes() {
        let mut project = default_project();
        let mut derived = Derived::default();
        assert!(derived.update(&project));
        assert!(!derived.update(&project));
        let id = project.components()[0].id();
        assert!(derived.main(id).is_some());
        assert!(derived.geometry(id).is_some());
        assert!(derived.impeller(id).unwrap().performance.is_ok());

        project.info.name = "Changed".into();
        assert!(derived.update(&project));
        assert_eq!(derived.revision(), 2);
    }

    #[test]
    fn a_drag_of_a_volute_recomputes_only_the_volute() {
        let mut project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let impeller = project.components()[0].id();
        let volute = project.components()[1].id();
        let mut input = project.volute_input(volute).unwrap();
        input.cutwater.incidence = tp_design::Param::Manual(2f64.to_radians());
        project.set_volute(volute, input).unwrap();
        // During the drag the impeller results stay those of before.
        let before = derived
            .impeller(impeller)
            .unwrap()
            .performance
            .as_ref()
            .unwrap()
            .design
            .head;
        assert!(derived.update_while(&project, true));
        assert!(derived.partial);
        let angle = derived.volute(volute).unwrap().design.cutwater_angle;
        let flow = derived.volute(volute).unwrap().design.cutwater_flow_angle;
        assert!((angle - flow - 2f64.to_radians()).abs() < 1e-12);
        assert_eq!(derived.revision(), 2);
        // Still dragging, nothing new: nothing to do.
        assert!(!derived.update_while(&project, true));
        // The drag ends: everything again.
        assert!(derived.update_while(&project, false));
        assert!(!derived.partial);
        assert_eq!(derived.revision(), 3);
        let after = derived
            .impeller(impeller)
            .unwrap()
            .performance
            .as_ref()
            .unwrap()
            .design
            .head;
        assert!(after.is_finite() && before.is_finite());
        // Other edits recompute everything even while dragging.
        project.info.name = "Changed".into();
        assert!(derived.update_while(&project, true));
        assert!(!derived.partial);
    }

    #[test]
    fn errors_are_kept_per_impeller() {
        let mut project = default_project();
        let mut setup = project.setup().clone();
        setup.fluid = "Unobtainium".into();
        project.set_setup(setup).unwrap();
        let mut derived = Derived::default();
        derived.update(&project);
        let id = project.components()[0].id();
        let results = derived.impeller(id).unwrap();
        assert!(results.main.is_err());
        assert!(results.geometry.is_err());
        assert!(derived.liquid.as_ref().unwrap().is_err());
    }
}
