//! The `.tpj` project file: pretty-printed JSON with a format tag and version.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Project, ProjectError};

/// File extension of project files, without the dot.
pub const FILE_EXTENSION: &str = "tpj";

/// Version written by this program. Older versions are migrated on load.
pub const FORMAT_VERSION: u32 = 2;

const FORMAT_ID: &str = "turbopump-project";

#[derive(Serialize)]
struct FileOut<'a> {
    format: &'static str,
    version: u32,
    project: &'a Project,
}

#[derive(Deserialize)]
struct FileHeader {
    format: String,
    version: u32,
}

/// Version 2 moves the neck of the shroud into the seals: every meridional input with
/// a `material` gets the seals of an earlier project (the front one by its
/// `neck_length`, no rear one, no wear rings), and a chamber seal clearance set by hand
/// becomes the clearance of the front seal.
fn migrate_v1_seals(project: &mut serde_json::Value) {
    use serde_json::Value;
    use tp_design::Param;
    use tp_design::impeller::seals::Seals;

    match project {
        Value::Object(map) => {
            if let Some(Value::Object(meridional)) = map.get_mut("meridional")
                && let Some(Value::Object(material)) = meridional.get_mut("material")
            {
                let neck = material
                    .remove("neck_length")
                    .and_then(|v| serde_json::from_value::<Param<f64>>(v).ok())
                    .unwrap_or_default();
                let mut seals = serde_json::to_value(Seals::legacy(neck)).expect("seals");
                if let Some(Value::Object(chambers)) = meridional.get_mut("chambers")
                    && let Some(clearance) = chambers.remove("seal_clearance")
                {
                    seals["front"]["clearance"] = clearance;
                }
                meridional.insert("seals".into(), seals);
            }
            for v in map.values_mut() {
                migrate_v1_seals(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(migrate_v1_seals),
        _ => {}
    }
}

impl Project {
    /// Serializes the project to the `.tpj` text format.
    pub fn to_json(&self) -> Result<String, ProjectError> {
        Ok(serde_json::to_string_pretty(&FileOut {
            format: FORMAT_ID,
            version: FORMAT_VERSION,
            project: self,
        })?)
    }

    /// Parses a project from the `.tpj` text format, migrating older versions.
    pub fn from_json(text: &str) -> Result<Project, ProjectError> {
        let mut value: serde_json::Value = serde_json::from_str(text)?;
        let header = FileHeader::deserialize(&value)?;
        if header.format != FORMAT_ID {
            return Err(ProjectError::WrongFormat(header.format));
        }
        if header.version > FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion {
                found: header.version,
                supported: FORMAT_VERSION,
            });
        }
        if header.version < 2 {
            migrate_v1_seals(&mut value["project"]);
        }
        let mut project: Project = serde_json::from_value(value["project"].take())?;
        project.repair_after_load()?;
        Ok(project)
    }

    /// Writes the project to `path`. The file is replaced atomically, so a crash while
    /// saving never leaves a truncated project behind.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        let path = path.as_ref();
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        fs::write(&tmp, self.to_json()?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Project, ProjectError> {
        Self::from_json(&fs::read_to_string(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::pump;
    use crate::{ComponentKind, DesignStep};
    use tp_design::acceptance::{AcceptanceInput, QualityClass};

    #[test]
    fn json_round_trip() {
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Volute, None).unwrap();
        p.component_mut(id)
            .unwrap()
            .complete(DesignStep::MainDimensions)
            .unwrap();
        p.display_units
            .set(tp_units::QuantityKind::Head, "ft")
            .unwrap();
        p.acceptance.class = tp_design::Param::Manual(QualityClass::G2);

        let text = p.to_json().unwrap();
        assert!(text.contains(r#""format": "turbopump-project""#));
        assert_eq!(Project::from_json(&text).unwrap(), p);
    }

    #[test]
    fn files_without_acceptance_settings_load_them_automatic() {
        let p = pump();
        let mut value: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        value["project"]
            .as_object_mut()
            .unwrap()
            .remove("acceptance");
        let loaded = Project::from_json(&value.to_string()).unwrap();
        assert_eq!(loaded.acceptance, AcceptanceInput::default());
    }

    #[test]
    fn file_round_trip() {
        let p = pump();
        let dir = std::env::temp_dir().join(format!("tp-project-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pump.tpj");
        p.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), p);
        assert!(!dir.join("pump.tpj.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_foreign_and_future_files() {
        let text = pump().to_json().unwrap();
        let future = text.replace(r#""version": 2"#, r#""version": 99"#);
        assert!(matches!(
            Project::from_json(&future),
            Err(ProjectError::UnsupportedVersion { found: 99, .. })
        ));
        let foreign = text.replace("turbopump-project", "something-else");
        assert!(matches!(
            Project::from_json(&foreign),
            Err(ProjectError::WrongFormat(_))
        ));
        assert!(matches!(
            Project::from_json("{"),
            Err(ProjectError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_hand_edited_invalid_composition() {
        let mut p = pump();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        value["project"]["components"][0]["kind"] = serde_json::json!({"type": "volute"});
        value["project"]["components"][1]["kind"] = serde_json::json!({"type": "volute"});
        let mut doubled = value["project"]["components"][1].clone();
        doubled["id"] = serde_json::json!(7);
        value["project"]["components"]
            .as_array_mut()
            .unwrap()
            .push(doubled);
        let err = Project::from_json(&value.to_string()).unwrap_err();
        assert!(matches!(err, ProjectError::VoluteNotAtEnd));
    }

    #[test]
    fn repairs_next_id_after_load() {
        let mut p = pump();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        value["project"]["next_id"] = serde_json::json!(1);
        let mut loaded = Project::from_json(&value.to_string()).unwrap();
        let new = loaded.add_component(ComponentKind::Impeller, None).unwrap();
        assert_ne!(new, loaded.components()[0].id());
    }

    /// A version-1 file whose only impeller has the geometry inputs of `edit`, with
    /// `neck` as the neck length of its material.
    fn version_1_file(neck: Option<f64>, chamber_clearance: Option<f64>) -> String {
        use tp_design::impeller::ImpellerGeometryInput;
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        p.set_impeller_geometry(id, ImpellerGeometryInput::default())
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        value["version"] = serde_json::json!(1);
        let meridional = &mut value["project"]["components"][0]["geometry"]["meridional"];
        assert!(meridional.is_object(), "geometry layout changed");
        meridional.as_object_mut().unwrap().remove("seals");
        if let Some(n) = neck {
            meridional["material"]["neck_length"] =
                serde_json::json!({"mode": "manual", "value": n});
        }
        if let Some(c) = chamber_clearance {
            meridional["chambers"]["seal_clearance"] =
                serde_json::json!({"mode": "manual", "value": c});
        }
        value.to_string()
    }

    fn seals_of(project: &Project) -> tp_design::impeller::seals::Seals {
        project.components()[0].geometry().unwrap().meridional.seals
    }

    #[test]
    fn a_version_1_file_gets_legacy_seals() {
        use tp_design::Param;
        let seals = seals_of(&Project::from_json(&version_1_file(Some(0.015), None)).unwrap());
        assert_eq!(seals.front.length, Param::Manual(0.015));
        assert!(seals.front.enabled);
        assert!(!seals.rear.enabled);
        assert!(!seals.front.impeller_ring.enabled && !seals.front.casing_ring.enabled);
        assert!(!seals.rear.impeller_ring.enabled && !seals.rear.casing_ring.enabled);
    }

    #[test]
    fn a_version_1_file_without_a_neck_keeps_the_automatic_one() {
        use tp_design::Param;
        let seals = seals_of(&Project::from_json(&version_1_file(None, None)).unwrap());
        assert!(seals.front.enabled);
        assert_eq!(seals.front.length, Param::Auto);
        assert!(!seals.front.impeller_ring.enabled && !seals.front.casing_ring.enabled);
    }

    #[test]
    fn a_version_1_neck_of_zero_turns_the_front_seal_off() {
        let seals = seals_of(&Project::from_json(&version_1_file(Some(0.0), None)).unwrap());
        assert!(!seals.front.enabled);
    }

    #[test]
    fn a_version_1_chamber_clearance_moves_to_the_front_seal() {
        use tp_design::Param;
        let seals = seals_of(&Project::from_json(&version_1_file(None, Some(0.0004))).unwrap());
        assert_eq!(seals.front.clearance, Param::Manual(0.0004));
    }

    #[test]
    fn a_version_2_file_keeps_its_seals() {
        use tp_design::impeller::ImpellerGeometryInput;
        let mut p = pump();
        let id = p.add_component(ComponentKind::Impeller, None).unwrap();
        let mut input = ImpellerGeometryInput::default();
        input.meridional.seals.rear.enabled = false;
        p.set_impeller_geometry(id, input).unwrap();
        assert!(
            !seals_of(&Project::from_json(&p.to_json().unwrap()).unwrap())
                .rear
                .enabled
        );
    }
}
