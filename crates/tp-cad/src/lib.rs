//! CAD solids of Turbopump designs with Open CASCADE (OCCT 7.8.1, built statically by
//! `occt-sys`), through a small C++ bridge (`cpp/tp_cad.cpp`) that turns every OCCT
//! exception into an error.
//!
//! The CAD model is built in **millimetres**, the default unit of the OCCT STEP and
//! IGES writers; volumes are reported in m³.

mod bridge;
mod diffuser;
mod impeller;
mod occ;
mod volute;

pub use diffuser::{DiffuserCad, build_diffuser};
pub use impeller::{BalanceHoleOptions, BalanceHoles, CadOptions, ImpellerCad, build_impeller};
pub use occ::{Shape, read_step, write_iges, write_step, write_stl};
pub use volute::{VoluteCad, VoluteCasing, build_volute, build_volute_casing};

/// Errors from CAD construction and export.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CadError {
    #[error("CAD operation failed: {0}")]
    Operation(String),

    #[error("invalid CAD input: {0}")]
    Input(String),

    #[error("{0}")]
    Io(String),
}
