/// Errors from design calculations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DesignError {
    /// An input value is out of its physical range.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The inputs are valid, but no geometry satisfies them.
    #[error("no feasible design: {0}")]
    Infeasible(String),
}
