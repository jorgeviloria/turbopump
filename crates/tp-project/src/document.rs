use std::path::{Path, PathBuf};

use crate::{History, Project, ProjectError};

const UNDO_LIMIT: usize = 200;

/// A project open in the editor: undo history, file path and unsaved-changes state.
#[derive(Debug)]
pub struct Document {
    project: Project,
    history: History<Project>,
    path: Option<PathBuf>,
    /// The project as last saved or loaded; `None` if it was never written.
    saved: Option<Project>,
}

impl Document {
    /// A new, unsaved document.
    pub fn new(project: Project) -> Self {
        Self {
            project,
            history: History::new(UNDO_LIMIT),
            path: None,
            saved: None,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let project = Project::load(path)?;
        Ok(Self {
            saved: Some(project.clone()),
            project,
            history: History::new(UNDO_LIMIT),
            path: Some(path.to_owned()),
        })
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn history(&self) -> &History<Project> {
        &self.history
    }

    /// Whether the project differs from what is on disk.
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.project)
    }

    /// Applies an edit as one undoable step named `label`. If `edit` fails, the project
    /// is restored to its previous state and nothing is recorded.
    pub fn edit<R>(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Result<R, ProjectError> {
        let before = self.project.clone();
        match edit(&mut self.project) {
            Ok(result) => {
                if self.project != before {
                    self.history.record(label, before);
                }
                Ok(result)
            }
            Err(e) => {
                self.project = before;
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo(&mut self.project)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo(&mut self.project)
    }

    pub fn save(&mut self) -> Result<(), ProjectError> {
        let path = self.path.clone().ok_or(ProjectError::NoPath)?;
        self.project.save(&path)?;
        self.saved = Some(self.project.clone());
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        self.path = Some(path.as_ref().to_owned());
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentKind;
    use crate::project::tests::pump;

    #[test]
    fn edits_are_undoable() {
        let mut doc = Document::new(pump());
        doc.edit("add impeller", |p| {
            p.add_component(ComponentKind::Impeller, None)
        })
        .unwrap();
        assert_eq!(doc.project().components().len(), 1);
        assert_eq!(doc.undo().as_deref(), Some("add impeller"));
        assert!(doc.project().components().is_empty());
        doc.redo();
        assert_eq!(doc.project().components().len(), 1);
    }

    #[test]
    fn failed_edits_roll_back() {
        let mut doc = Document::new(pump());
        let err = doc.edit("bad", |p| {
            p.info.name = "changed".into();
            p.add_component(ComponentKind::Impeller, None)?;
            p.add_component(ComponentKind::Volute, None)?;
            // A stator after the outlet volute breaks the composition rules.
            p.add_component(ComponentKind::Stator { vaned: true }, None)
        });
        assert!(err.is_err());
        assert_eq!(doc.project().info.name, "Test pump");
        assert!(doc.project().components().is_empty());
        assert!(!doc.history().can_undo());
    }

    #[test]
    fn no_op_edits_leave_no_history() {
        let mut doc = Document::new(pump());
        doc.edit("nothing", |_| Ok(())).unwrap();
        assert!(!doc.history().can_undo());
    }

    #[test]
    fn dirty_tracking_follows_saves_and_undo() {
        let dir = std::env::temp_dir().join(format!("tp-document-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.tpj");

        let mut doc = Document::new(pump());
        assert!(doc.is_dirty());
        assert!(matches!(doc.save(), Err(ProjectError::NoPath)));
        doc.save_as(&path).unwrap();
        assert!(!doc.is_dirty());

        doc.edit("rename", |p| {
            p.info.name = "Renamed".into();
            Ok(())
        })
        .unwrap();
        assert!(doc.is_dirty());
        doc.undo();
        assert!(!doc.is_dirty());

        let reopened = Document::open(&path).unwrap();
        assert_eq!(reopened.project(), doc.project());
        assert!(!reopened.is_dirty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
