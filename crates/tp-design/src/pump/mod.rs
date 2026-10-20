//! Radial (centrifugal) pumps.

pub mod correlations;
mod main_dimensions;

pub use main_dimensions::{
    CentrifugalPumpDesign, CentrifugalPumpInput, Efficiencies, InletSection, OutletSection,
    PumpDuty, Suction, design,
};
