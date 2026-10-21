//! Geometry and numerics for Turbopump.
//!
//! Pure Rust, no native dependencies: everything here can be tested without a CAD
//! kernel. Curves are generic over [`Point`], so the same code serves 1D distributions
//! (`f64`), 2D design diagrams ([`DVec2`]) and 3D geometry ([`DVec3`]).

mod bezier;
mod bspline;
mod error;
mod interp;
pub mod linalg;
pub mod optim;
mod point;
mod polyline;
pub mod quad;
pub mod roots;
pub mod sparse;

pub use bezier::Bezier;
pub use bspline::BSpline;
pub use error::MathError;
pub use glam::{DVec2, DVec3};
pub use interp::Table1D;
pub use point::Point;
pub use polyline::Polyline;
