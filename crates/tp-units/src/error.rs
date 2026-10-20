use crate::QuantityKind;

/// Errors produced while parsing or converting quantities.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UnitError {
    #[error("invalid number in {0:?}")]
    InvalidNumber(String),

    #[error("missing unit in {text:?}: expected a {kind} unit such as {example:?}")]
    MissingUnit {
        text: String,
        kind: QuantityKind,
        example: &'static str,
    },

    #[error("unknown {kind} unit {symbol:?}; known units: {known}")]
    UnknownUnit {
        symbol: String,
        kind: QuantityKind,
        known: String,
    },

    #[error("unknown unit {0:?}")]
    UnknownSymbol(String),

    #[error("cannot convert {from:?} to {to:?}: they measure different quantities")]
    Incompatible { from: String, to: String },
}
