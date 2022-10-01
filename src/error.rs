use std::fmt;

#[derive(Debug)]
pub enum ArchiveError {
    Io(std::io::Error),
    Request(reqwest::Error),
    Database(rusqlite::Error),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ArchiveError::Io(ref err) => err.fmt(f),
            ArchiveError::Request(ref err) => err.fmt(f),
            ArchiveError::Database(ref err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for ArchiveError {
    fn from(err: std::io::Error) -> ArchiveError {
        ArchiveError::Io(err)
    }
}

impl From<reqwest::Error> for ArchiveError {
    fn from(err: reqwest::Error) -> ArchiveError {
        ArchiveError::Request(err)
    }
}

impl From<rusqlite::Error> for ArchiveError {
    fn from(err: rusqlite::Error) -> ArchiveError {
        ArchiveError::Database(err)
    }
}
