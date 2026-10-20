use crate::{ComponentId, DesignStep};

/// Errors from editing, loading or saving a project.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("a project can hold at most {max} impellers")]
    TooManyImpellers { max: usize },

    #[error("a volute can only be the first or the last component")]
    VoluteNotAtEnd,

    #[error("position {position} is beyond the {len} components of the project")]
    InvalidPosition { position: usize, len: usize },

    #[error("no component with id {0}")]
    UnknownComponent(ComponentId),

    #[error("{step} is not a design step of this component")]
    StepNotApplicable { step: DesignStep },

    #[error("{step} needs {previous} to be completed first")]
    PreviousStepIncomplete {
        step: DesignStep,
        previous: DesignStep,
    },

    #[error("invalid global setup: {0}")]
    InvalidSetup(String),

    #[error("{0} is not an impeller")]
    NotAnImpeller(ComponentId),

    #[error("{0} is not a volute")]
    NotAVolute(ComponentId),

    #[error("{0} is not a stator")]
    NotAStator(ComponentId),

    #[error("not available yet: {0}")]
    Unsupported(String),

    #[error(transparent)]
    Fluid(#[from] tp_fluids::FluidError),

    #[error(transparent)]
    Design(#[from] tp_design::DesignError),

    #[error("not a Turbopump project file (format {0:?})")]
    WrongFormat(String),

    #[error(
        "project file version {found} is newer than this program supports ({supported}); \
         please update Turbopump"
    )]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("malformed project file: {0}")]
    Malformed(#[from] serde_json::Error),

    #[error("the document has no file path; use save-as")]
    NoPath,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
