//! Graphical editor of Turbopump.
//!
//! The editor walks the user through the design steps of each component: a toolbar on
//! top, the components and the project tree on the left, the inputs of the selected
//! step on the right, and dockable views (meridional contour, blade plots, front view,
//! 3D view and report) in the middle.
//! Every edit goes through [`tp_project::Document`], so it can be undone, and every
//! result comes from the same design code as the command line.

mod app;
mod derived;
mod diffuser_steps;
mod export;
mod i18n;
mod icons;
mod messages;
mod ribbon;
mod section_editor;
mod steps;
mod theme;
mod tree;
mod viewer;
mod views;
mod volute_steps;
mod widgets;

use std::f64::consts::PI;

pub use app::{Selection, TurbopumpApp, View};
use tp_project::{
    ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
};

/// The project a new window starts with: a single-stage water pump for 100 m³/h and
/// 32 m at 2900 rpm, with one impeller and its volute.
pub fn default_project() -> Project {
    let setup = GlobalSetup::new(
        MachineType::Pump,
        DesignPoint {
            flow: FlowRate::Volume(100.0 / 3600.0),
            energy: EnergyTransfer::Head(32.0),
            speed: 2900.0 * PI / 30.0,
        },
    );
    let mut project =
        Project::new("Untitled", MachineType::Pump, setup).expect("the default setup is valid");
    project
        .add_component(ComponentKind::Impeller, None)
        .expect("a pump takes an impeller");
    project
        .add_component(ComponentKind::Volute, None)
        .expect("a volute closes the pump");
    project
}
