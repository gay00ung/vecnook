use std::{fmt, io};

/// Convenience result type for Vecnook operations.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
/// Input, corruption, storage and ownership failures.
/// I/O failure on a mutation can be ambiguous; close and reopen before writing again.
pub enum Error {
    /// Caller input violates dimensions, finite values or documented limits.
    InvalidInput(String),
    /// Authoritative file or in-memory invariant validation failed.
    Corrupt(String),
    /// Filesystem or synchronization failure; a write may have persisted.
    Io(io::Error),
    /// Another handle/process holds the database exclusive OS lock.
    Locked,
    /// Create or backup refuses an existing destination.
    AlreadyExists,
    /// A prior storage failure requires closing and reopening before writing.
    Poisoned,
    /// Persistent operations are unavailable on this platform.
    UnsupportedPlatform,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(s) => write!(f, "invalid input: {s}"),
            Self::Corrupt(s) => write!(f, "corrupt database: {s}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Locked => write!(
                f,
                "database is already open; close the other handle/process"
            ),
            Self::AlreadyExists => write!(
                f,
                "database files already exist; init will not overwrite them"
            ),
            Self::Poisoned => write!(
                f,
                "a previous storage failure requires closing and reopening this database"
            ),
            Self::UnsupportedPlatform => {
                write!(f, "persistent storage currently supports macOS/Linux")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
