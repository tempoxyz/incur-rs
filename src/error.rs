/// Field-level validation error detail.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldError {
    pub path: String,
    pub expected: String,
    pub received: String,
    pub message: String,
}

/// All error types produced by incur.
#[derive(Debug, thiserror::Error)]
pub enum IncurError {
    #[error("{message}")]
    Parse { message: String },

    #[error("{message}")]
    Validation {
        message: String,
        field_errors: Vec<FieldError>,
    },

    #[error("{message}")]
    Command {
        code: String,
        message: String,
        retryable: bool,
    },

    #[error("'{name}' is not a command. See '{help_cmd}' for available commands.")]
    CommandNotFound { name: String, help_cmd: String },
}

pub type IncurResult<T> = Result<T, IncurError>;
