//! CAD solids of Turbopump designs with Open CASCADE (OCCT 8.0.1, built statically by
//! `tp-occt-sys`), through a small C++ bridge (`cpp/tp_cad.cpp`) that turns every OCCT
//! exception into an error.
//!
//! The CAD model is built in **millimetres**, the default unit of the OCCT STEP and
//! IGES writers; volumes are reported in m³.

mod bridge;
mod curves;
mod diffuser;
mod impeller;
mod joined;
mod occ;
mod patches;
mod profile;
mod rounding;
mod sewn;
mod thin;
mod volute;

pub use curves::{VoluteSurfaces, build_volute_surfaces};
pub use diffuser::{DiffuserCad, build_diffuser};
pub use impeller::{
    BalanceHoleOptions, BalanceHoles, CadOptions, ImpellerCad, RingKind, WearRing, build_impeller,
    build_inlet_duct,
};
pub use joined::{join_fluid, joining_ring};
pub use occ::{Shape, read_step, write_iges, write_step, write_stl};
pub use patches::{Patch, impeller_patches, volute_patches, write_patches};
pub use sewn::{FlowBuild, build_volute_flow, build_volute_sewn};
pub use volute::{
    VoluteCad, VoluteCasing, build_volute, build_volute_casing, expected_fluid_volume,
};

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

/// At most two CAD tests build at once on the machine, across test threads, test
/// binaries and checkouts: cargo runs one test per core, and each holds large shapes
/// and runs OCCT on several threads. All at once froze a 16 GB laptop.
#[cfg(test)]
pub(crate) mod test_slot {
    use std::fs::File;
    use std::time::Duration;

    const SLOTS: usize = 2;

    /// Holds a slot until dropped (the lock goes with the file).
    pub(crate) struct Slot {
        _lock: Option<File>,
    }

    pub(crate) fn take() -> Slot {
        loop {
            for k in 0..SLOTS {
                let path = std::env::temp_dir().join(format!("turbopump-cad-slot-{k}.lock"));
                let Ok(file) = File::options()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                else {
                    // No lock files here: run unthrottled rather than fail.
                    return Slot { _lock: None };
                };
                if file.try_lock().is_ok() {
                    return Slot { _lock: Some(file) };
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
