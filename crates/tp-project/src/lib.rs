//! The Turbopump project model.
//!
//! A [`Project`] describes one machine: its type, global design point and an ordered
//! list of [`Component`]s (impellers, stators, volutes). Each component walks through a
//! chain of [`DesignStep`]s; changing an upstream input marks downstream steps stale.
//! A [`Document`] wraps a project with undo/redo and save state for the editor.

mod component;
mod design;
mod document;
mod error;
mod file;
mod history;
mod machine;
mod project;
mod setup;
pub mod validation;

pub use component::{
    AcceptedRows, AcceptedValue, Component, ComponentId, ComponentKind, Coupling, DesignStep,
    StepStatus,
};
pub use design::{Designed, GEOMETRY_STEPS, VANED_STEPS, VANELESS_STEPS, VOLUTE_STEPS, can_design};
pub use document::Document;
pub use error::ProjectError;
pub use file::{FILE_EXTENSION, FORMAT_VERSION};
pub use history::History;
pub use machine::MachineType;
pub use project::{Issue, Project, ProjectInfo, Severity};
pub use setup::{DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, InletConditions, Rotation};
pub use tp_design::Param;
