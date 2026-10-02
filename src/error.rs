use std::{fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    InvalidInput(String),
    Corrupt(String),
    Io(io::Error),
    Locked,
    AlreadyExists,
    Poisoned,
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
