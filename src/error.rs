use std::error;
use std::fmt;

#[derive(Debug)]
pub enum ArchiveError {
    ContentBuild(&'static str),
    StoryBuild(&'static str),
    Io(std::io::Error),
    Request(reqwest::Error),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ArchiveError::ContentBuild(str) => write!(f, "{}", str),
            ArchiveError::StoryBuild(str) => write!(f, "{}", str),
            ArchiveError::Io(ref err) => err.fmt(f),
            ArchiveError::Request(ref err) => err.fmt(f),
        }
    }
}

impl error::Error for ArchiveError {
    fn description(&self) -> &str {
        match *self {
            ArchiveError::ContentBuild(str) => str,
            ArchiveError::StoryBuild(str) => str,
            ArchiveError::Io(ref err) => err.description(),
            ArchiveError::Request(ref err) => err.description(),
        }
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        match &*self {
            ArchiveError::ContentBuild(_) => None,
            ArchiveError::StoryBuild(_) => None,
            ArchiveError::Io(ref err) => Some(err),
            ArchiveError::Request(ref err) => Some(err),
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
