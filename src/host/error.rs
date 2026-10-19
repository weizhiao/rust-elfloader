use alloc::{boxed::Box, string::String};
use core::fmt;

/// Errors returned when importing a library managed by the native loader.
#[derive(Debug)]
pub enum HostError {
    /// A native loader operation failed.
    Native {
        /// Operation that failed.
        operation: &'static str,
        /// Diagnostic copied from the native loader.
        message: String,
    },
    /// The file cannot provide the metadata needed for an import.
    Metadata {
        /// Description of the missing or invalid metadata.
        message: String,
    },
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native { operation, message } => write!(f, "{operation}: {message}"),
            Self::Metadata { message } => write!(f, "invalid host ELF metadata: {message}"),
        }
    }
}

impl core::error::Error for HostError {}

impl From<HostError> for crate::Error {
    fn from(error: HostError) -> Self {
        Self::Host(Box::new(error))
    }
}

pub(super) fn metadata(error: impl fmt::Display) -> crate::Error {
    HostError::Metadata {
        message: alloc::format!("{error}"),
    }
    .into()
}
