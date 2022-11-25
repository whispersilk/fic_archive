use std::{error::Error, fmt};

#[derive(Debug)]
pub enum ArchiveError {
    Internal(String),
    BadSource(String),
    NoIdInSource(String, String),
    PageError(String),
    StoryNotExists(String),
    Io(std::io::Error),
    Request(reqwest::Error),
    Database(rusqlite::Error),
    Parse(chrono::format::ParseError),
    ParseInt(std::num::ParseIntError),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Internal(ref s) => write!(f, "Internal error: {}", s),
            Self::BadSource(ref s) => write!(f, "Could not convert URL {} to a story source", s),
            Self::NoIdInSource(ref url, ref name) => write!(
                f,
                "Url {url} maps to source {name} and must contain a story ID, but does not"
            ),
            Self::PageError(ref s) => write!(f, "{}", s),
            Self::StoryNotExists(ref s) => write!(
                f,
                "Story {} does not exist in the archive. Try adding it first.",
                s
            ),
            Self::Io(ref err) => err.fmt(f),
            Self::Request(ref err) => err.fmt(f),
            Self::Database(ref err) => err.fmt(f),
            Self::Parse(ref err) => err.fmt(f),
            Self::ParseInt(ref err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for ArchiveError {
    fn from(err: std::io::Error) -> ArchiveError {
        Self::Io(err)
    }
}

impl From<reqwest::Error> for ArchiveError {
    fn from(err: reqwest::Error) -> ArchiveError {
        Self::Request(err)
    }
}

impl From<rusqlite::Error> for ArchiveError {
    fn from(err: rusqlite::Error) -> ArchiveError {
        Self::Database(err)
    }
}

impl From<chrono::format::ParseError> for ArchiveError {
    fn from(err: chrono::format::ParseError) -> ArchiveError {
        Self::Parse(err)
    }
}

impl From<std::num::ParseIntError> for ArchiveError {
    fn from(err: std::num::ParseIntError) -> ArchiveError {
        Self::ParseInt(err)
    }
}

impl Error for ArchiveError {}
