//! Turbomachinery design methods.
//!
//! Calculations take plain SI inputs and return SI results; they know nothing about
//! projects or files, so the same code serves the editor, the command line and tests.

pub mod diffuser;
mod error;
pub mod impeller;
pub mod outlet;
mod param;
pub mod performance;
pub mod pump;
pub mod slip;
pub mod triangle;
pub mod volute;

use serde::{Deserialize, Serialize};

pub use error::DesignError;
pub use param::Param;

/// Inputs of the main-dimensions step of an impeller, by design method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum MainDimensionsInput {
    CentrifugalPump(pump::CentrifugalPumpInput),
}
