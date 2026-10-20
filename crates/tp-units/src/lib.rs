//! Physical quantities and units for Turbopump.
//!
//! Every value inside Turbopump is stored in **SI base units** (m, kg, s, K, rad, ...).
//! User-facing units only appear at the boundary: when text is parsed
//! ([`parse_quantity`]) and when values are shown ([`DisplayUnits`]).
//!
//! ```
//! use tp_units::{QuantityKind, parse_quantity, SpecificSpeed};
//!
//! let q = parse_quantity("100 m3/h", QuantityKind::VolumeFlow).unwrap();
//! let h = parse_quantity("32 m", QuantityKind::Head).unwrap();
//! let n = parse_quantity("2900 rpm", QuantityKind::RotationalSpeed).unwrap();
//! let nq = SpecificSpeed::from_head(q, h, n).nq();
//! assert!((nq - 35.9).abs() < 0.1);
//! ```

mod display;
mod error;
mod kind;
mod parse;
mod specific_speed;
mod unit;

pub use display::{DisplayUnits, decimals_for};
pub use error::UnitError;
pub use kind::QuantityKind;
pub use parse::{parse_quantity, parse_quantity_or};
pub use specific_speed::SpecificSpeed;
pub use unit::{Unit, convert, find_unit};

/// Standard acceleration of gravity g_n in m/s² (CGPM 1901, exact by definition).
pub const STANDARD_GRAVITY: f64 = 9.806_65;
