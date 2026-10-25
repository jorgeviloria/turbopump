use std::path::{Path, PathBuf};

use crate::{History, Project, ProjectError};

const UNDO_LIMIT: usize = 200;

/// A project open in the editor: undo history, file path and unsaved-changes state.
#[derive(Debug)]
pub struct Document {
    project: Project,
    history: History<Project>,
    path: Option<PathBuf>,
    /// The project with no changes: as last saved or loaded, or as created for a new
    /// document, so that an untouched new project has nothing to save.
    clean: Project,
    /// Label of the gesture whose edits are merging into the last undo step.
    gesture: Option<String>,
}

impl Document {
    /// A new document, not yet written to a file. Until it is edited it has no
    /// unsaved changes.
    pub fn new(project: Project) -> Self {
        Self {
            clean: project.clone(),
            project,
            history: History::new(UNDO_LIMIT),
            path: None,
            gesture: None,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let project = Project::load(path)?;
        Ok(Self {
            clean: project.clone(),
            project,
            history: History::new(UNDO_LIMIT),
            path: Some(path.to_owned()),
            gesture: None,
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

    /// Whether the project has changes to save: it differs from what is on disk, or,
    /// for a new document, from the project it was created with.
    pub fn is_dirty(&self) -> bool {
        self.clean != self.project
    }

    /// Applies an edit as one undoable step named `label`. If `edit` fails, the project
    /// is restored to its previous state and nothing is recorded.
    pub fn edit<R>(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Result<R, ProjectError> {
        self.gesture = None;
        self.apply(label, false, edit)
    }

    /// Like [`edit`](Self::edit), for continuous gestures such as dragging a point or a
    /// value: consecutive edits with the same `label` merge into one undo step until
    /// [`end_gesture`](Self::end_gesture), another kind of edit, an undo or a redo.
    pub fn edit_gesture<R>(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Result<R, ProjectError> {
        let merge = self.gesture.as_deref() == Some(label);
        if !merge {
            self.gesture = None;
        }
        let result = self.apply(label, merge, edit);
        if result.is_ok() && !merge && self.history.undo_label() == Some(label) {
            self.gesture = Some(label.to_owned());
        }
        result
    }

    /// Notes something about the project that is not an edit, such as what the rows of
    /// an accepted step showed: no undo step, and a project with nothing to save keeps
    /// nothing to save (the note is written with the next save).
    pub fn annotate(&mut self, note: impl FnOnce(&mut Project)) {
        let clean = !self.is_dirty();
        note(&mut self.project);
        if clean {
            self.clean = self.project.clone();
        }
    }

    /// Ends the current gesture: the next edit starts a new undo step.
    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    fn apply<R>(
        &mut self,
        label: &str,
        merge: bool,
        edit: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Result<R, ProjectError> {
        let before = self.project.clone();
        match edit(&mut self.project) {
            Ok(result) => {
                if self.project != before && !merge {
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
        self.gesture = None;
        self.history.undo(&mut self.project)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.gesture = None;
        self.history.redo(&mut self.project)
    }

    pub fn save(&mut self) -> Result<(), ProjectError> {
        let path = self.path.clone().ok_or(ProjectError::NoPath)?;
        self.project.save(&path)?;
        self.clean = self.project.clone();
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
    fn notes_are_neither_edits_nor_unsaved_changes() {
        use crate::{AcceptedValue, DesignStep};
        let mut doc = Document::new(pump());
        doc.edit("add impeller", |p| {
            p.add_component(ComponentKind::Impeller, None)
        })
        .unwrap();
        let id = doc.project().components()[0].id();
        let note = |doc: &mut Document, text: &str| {
            let row = AcceptedValue {
                key: text.into(),
                text: text.into(),
                quantity: None,
                automatic: true,
            };
            doc.annotate(|p| {
                p.component_mut(id).unwrap().set_accepted(
                    DesignStep::MainDimensions,
                    [("Dimensions › d₂".to_owned(), row)].into(),
                );
            });
        };
        // On unsaved changes: still unsaved, and no undo step of its own.
        note(&mut doc, "172");
        assert!(doc.is_dirty());
        assert_eq!(doc.history().undo_label(), Some("add impeller"));
        // On a project with nothing to save: still nothing to save.
        let mut clean = Document::new(doc.project().clone());
        note(&mut clean, "180");
        assert!(!clean.is_dirty());
        let kept = clean.project().components()[0]
            .accepted(DesignStep::MainDimensions)
            .unwrap();
        assert_eq!(kept["Dimensions › d₂"].text, "180");
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
    fn gestures_merge_into_one_undo_step() {
        let mut doc = Document::new(pump());
        let rename = |doc: &mut Document, name: &str| {
            doc.edit_gesture("rename", |p| {
                p.info.name = name.into();
                Ok(())
            })
            .unwrap();
        };
        rename(&mut doc, "A");
        rename(&mut doc, "AB");
        rename(&mut doc, "ABC");
        assert_eq!(doc.undo().as_deref(), Some("rename"));
        assert_eq!(doc.project().info.name, "Test pump");
        assert!(!doc.history().can_undo());

        // Ending the gesture, or a different label, starts a new step.
        doc.redo();
        rename(&mut doc, "D");
        doc.end_gesture();
        rename(&mut doc, "E");
        doc.edit_gesture("other", |p| {
            p.info.author = "me".into();
            Ok(())
        })
        .unwrap();
        doc.undo();
        doc.undo();
        assert_eq!(doc.project().info.name, "D");
        doc.undo();
        assert_eq!(doc.project().info.name, "ABC");
    }

    #[test]
    fn dirty_tracking_follows_saves_and_undo() {
        let dir = std::env::temp_dir().join(format!("tp-document-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.tpj");

        // An untouched new project has nothing to save; an edit changes that.
        let mut doc = Document::new(pump());
        assert!(!doc.is_dirty());
        doc.edit("rename", |p| {
            p.info.name = "First".into();
            Ok(())
        })
        .unwrap();
        assert!(doc.is_dirty());
        doc.undo();
        assert!(!doc.is_dirty());
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
