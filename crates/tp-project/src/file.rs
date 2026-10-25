//! The `.tpj` project file: pretty-printed JSON with a format tag and version.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Project, ProjectError};

/// File extension of project files, without the dot.
pub const FILE_EXTENSION: &str = "tpj";

/// Version written by this program. Older versions are migrated on load.
pub const FORMAT_VERSION: u32 = 1;

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
        // Version 1 is the first format; migrations from older versions go here.
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
        let future = text.replace(r#""version": 1"#, r#""version": 99"#);
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
}
