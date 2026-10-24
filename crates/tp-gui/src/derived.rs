//! Results computed from the project, recomputed only when the project changes.

use std::collections::BTreeMap;

use tp_design::diffuser::DiffuserDesign;
use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::duct::{self, InletDuct};
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
    /// Inlet ducts, with the impeller after each.
    pub ducts: BTreeMap<ComponentId, (ComponentId, Result<InletDuct, String>)>,
}

impl Derived {
    /// Recomputes the results if `project` differs from the one they were computed for.
    /// Returns whether anything was recomputed.
    pub fn update(&mut self, project: &Project) -> bool {
        if self.source.as_ref() == Some(project) {
            return false;
        }
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
        // Each duct on the geometry of the impeller after it, as just built.
        self.ducts = project
            .components()
            .iter()
            .filter(|c| c.kind() == ComponentKind::InletDuct)
            .filter_map(|c| {
                let impeller = project.impeller_after_duct(c.id()).ok()?;
                let built = match (
                    project.duct_input(c.id()),
                    self.impellers.get(&impeller).map(|r| &r.geometry),
                ) {
                    (Ok(input), Some(Ok(g))) => Ok(duct::build(&g.contour, &g.basis, &input)),
                    (Err(e), _) => Err(e.to_string()),
                    _ => Err("the impeller after the duct could not be built".into()),
                };
                Some((c.id(), (impeller, built)))
            })
            .collect();
        self.source = Some(project.clone());
        self.revision += 1;
        true
    }

    pub fn duct_result(&self, id: ComponentId) -> Option<&Result<InletDuct, String>> {
        self.ducts.get(&id).map(|(_, r)| r)
    }

    pub fn duct(&self, id: ComponentId) -> Option<&InletDuct> {
        self.duct_result(id).and_then(|r| r.as_ref().ok())
    }

    /// The inlet duct component ahead of impeller `impeller`, if there is one.
    pub fn duct_id_before(&self, impeller: ComponentId) -> Option<ComponentId> {
        self.ducts
            .iter()
            .find(|(_, (after, _))| *after == impeller)
            .map(|(id, _)| *id)
    }

    /// The inlet duct ahead of impeller `impeller`, when it was built.
    pub fn duct_before(&self, impeller: ComponentId) -> Option<&InletDuct> {
        self.ducts
            .values()
            .find(|(after, _)| *after == impeller)
            .and_then(|(_, r)| r.as_ref().ok())
            .filter(|d| !d.outline.is_empty())
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
