use std::fmt;

/// Coarse classification of an error, used by transport layers to pick a
/// status code without the domain knowing anything about HTTP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    Invalid,
    Conflict,
    VersionConflict,
    Unauthorized,
    Forbidden,
    Unavailable,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("version conflict: expected {expected}, current {current}")]
    VersionConflict { expected: i64, current: i64 },

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("search error: {0}")]
    Search(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("upstream unavailable: {0}")]
    Unavailable(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// The message without the classifying prefix.
    ///
    /// `Display` names the kind because a log line has no other context. A
    /// surface that already shows the kind — a queue row with a "failed"
    /// badge, say — would otherwise read "failed / invalid input: …".
    pub fn detail(&self) -> String {
        match self {
            Error::NotFound(m)
            | Error::Invalid(m)
            | Error::Conflict(m)
            | Error::Forbidden(m)
            | Error::Storage(m)
            | Error::Search(m)
            | Error::Plugin(m)
            | Error::Unavailable(m)
            | Error::Internal(m) => m.clone(),
            other => other.to_string(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::NotFound(_) => ErrorKind::NotFound,
            Error::Invalid(_) => ErrorKind::Invalid,
            Error::Conflict(_) => ErrorKind::Conflict,
            Error::VersionConflict { .. } => ErrorKind::VersionConflict,
            Error::Unauthorized => ErrorKind::Unauthorized,
            Error::Forbidden(_) => ErrorKind::Forbidden,
            Error::Unavailable(_) => ErrorKind::Unavailable,
            Error::Storage(_) | Error::Search(_) | Error::Plugin(_) | Error::Internal(_) => {
                ErrorKind::Internal
            }
        }
    }

    /// Stable machine-readable code, surfaced in API error payloads.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::Invalid(_) => "invalid_input",
            Error::Conflict(_) => "conflict",
            Error::VersionConflict { .. } => "version_conflict",
            Error::Unauthorized => "unauthorized",
            Error::Forbidden(_) => "forbidden",
            Error::Storage(_) => "storage_error",
            Error::Search(_) => "search_error",
            Error::Plugin(_) => "plugin_error",
            Error::Unavailable(_) => "unavailable",
            Error::Internal(_) => "internal_error",
        }
    }

    pub fn invalid(msg: impl fmt::Display) -> Self {
        Error::Invalid(msg.to_string())
    }
    pub fn not_found(msg: impl fmt::Display) -> Self {
        Error::NotFound(msg.to_string())
    }
    pub fn internal(msg: impl fmt::Display) -> Self {
        Error::Internal(msg.to_string())
    }
    pub fn storage(msg: impl fmt::Display) -> Self {
        Error::Storage(msg.to_string())
    }
    pub fn plugin(msg: impl fmt::Display) -> Self {
        Error::Plugin(msg.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Invalid(format!("json: {e}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Internal(format!("io: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
