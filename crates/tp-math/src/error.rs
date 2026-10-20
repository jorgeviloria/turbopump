/// Errors from geometric construction and numerical algorithms.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MathError {
    #[error("need at least {needed} points, got {got}")]
    TooFewPoints { needed: usize, got: usize },

    #[error("invalid knot vector: {0}")]
    InvalidKnots(String),

    #[error("x values must be finite and strictly increasing")]
    NotIncreasing,

    #[error("consecutive points coincide; cannot parameterize the curve")]
    CoincidentPoints,

    #[error("value {value} lies outside [{min}, {max}]")]
    OutOfRange { value: f64, min: f64, max: f64 },

    #[error("f(a) and f(b) have the same sign on [{a}, {b}]; the root is not bracketed")]
    NotBracketed { a: f64, b: f64 },

    #[error("no convergence after {0} iterations")]
    NoConvergence(usize),

    #[error("matrix is singular")]
    Singular,
}
