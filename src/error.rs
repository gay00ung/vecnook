use std::{fmt, io};

/// Convenience result type for Vecnook operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Machine-readable failure categories. Match with a wildcard for future additions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Caller supplied invalid data or settings.
    InvalidInput,
    /// A bounded storage or batch resource would be exceeded.
    Capacity,
    /// Model identity, dimensions or metric differ from the collection.
    EmbeddingMismatch,
    /// Authoritative data failed validation.
    Corrupt,
    /// Filesystem failure; inspect the original I/O source.
    Io,
    /// Another process owns this database.
    Locked,
    /// The requested destination already exists.
    AlreadyExists,
    /// A previous write failure requires reopening.
    Poisoned,
    /// Persistent storage is unavailable on this platform.
    UnsupportedPlatform,
}

#[derive(Debug)]
/// Input, corruption, storage and ownership failures.
/// I/O failure on a mutation can be ambiguous; close and reopen before writing again.
#[non_exhaustive]
pub enum Error {
    /// Caller input violates dimensions, finite values or documented limits.
    InvalidInput(String),
    /// Preflight rejection; no mutation was attempted. Compact or reduce the batch.
    Capacity {
        /// Bounded resource name.
        resource: &'static str,
        /// Maximum permitted value, in bytes or entries according to the resource.
        limit: usize,
        /// Value required by the proposed operation.
        required: usize,
    },
    /// Open was given a different model, dimension or metric identity.
    EmbeddingMismatch,
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

impl Error {
    /// Classify errors without parsing display text.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidInput(_) => ErrorKind::InvalidInput,
            Self::Capacity { .. } => ErrorKind::Capacity,
            Self::EmbeddingMismatch => ErrorKind::EmbeddingMismatch,
            Self::Corrupt(_) => ErrorKind::Corrupt,
            Self::Io(_) => ErrorKind::Io,
            Self::Locked => ErrorKind::Locked,
            Self::AlreadyExists => ErrorKind::AlreadyExists,
            Self::Poisoned => ErrorKind::Poisoned,
            Self::UnsupportedPlatform => ErrorKind::UnsupportedPlatform,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(s) => write!(f, "invalid input: {s}"),
            Self::Capacity {
                resource,
                limit,
                required,
            } => write!(
                f,
                "capacity: {resource} needs {required}, limit {limit}; compact or reduce the operation"
            ),
            Self::EmbeddingMismatch => {
                f.write_str("embedding space mismatch: model, dimensions and metric must match")
            }
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
                write!(f, "persistent storage supports macOS/Linux/Windows")
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
