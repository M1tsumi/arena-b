use std::fmt;

/// Crate-level error type for arena-b
#[derive(Debug)]
pub enum ArenaError {
    /// Allocation or initialization failed with message
    AllocationFailed(String),
    /// Virtual memory related error
    VirtualMemoryError(String),
    /// Layout computation failed
    InvalidLayout(String),
    /// Generic error wrapper
    Other(String),
}

impl fmt::Display for ArenaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArenaError::AllocationFailed(s) => write!(f, "Allocation failed: {}", s),
            ArenaError::VirtualMemoryError(s) => write!(f, "Virtual memory error: {}", s),
            ArenaError::InvalidLayout(s) => write!(f, "Invalid layout: {}", s),
            ArenaError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for ArenaError {}
