use chrono::{DateTime, FixedOffset};
use once_cell::sync::OnceCell;
use regex::Regex;

use crate::error::ArchiveError;
use crate::parser::{
    ao3::AO3Parser, katalepsis::KatalepsisParser, royalroad::RoyalRoadParser,
    xenforo::XenforoParser, Parser,
};
use crate::Result;

#[derive(Debug, Clone)]
pub enum Completed {
    Complete,
    Incomplete,
    Unknown,
}

impl Completed {
    pub fn to_string(&self) -> String {
        match self {
            Self::Complete => "COMPLETE".to_owned(),
            Self::Incomplete => "INCOMPLETE".to_owned(),
            Self::Unknown => "UNKNOWN".to_owned(),
        }
    }
    pub fn from_string(s: &str) -> Self {
        match s {
            "COMPLETE" => Self::Complete,
            "INCOMPLETE" => Self::Incomplete,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Story {
    pub name: String,
    pub authors: AuthorList,
    pub description: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
    pub chapters: Vec<Content>,
    pub source: StorySource,
    pub completed: Completed,
}

impl Story {
    pub fn num_chapters(&self) -> usize {
        self.chapters.iter().fold(0, |acc, con| match con {
            Content::Section(sec) => acc + sec.num_chapters(),
            Content::Chapter(_) => acc + 1,
        })
    }

    pub fn find_chapter(&self, id: String) -> Option<FindChapter> {
        self.chapters.iter().find_map(|con| {
            if con.id() == &id {
                Some(FindChapter {
                    chapter: con,
                    parent: None,
                })
            } else if let Content::Section(_) = con {
                con.find_child(&id)
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListedStory {
    pub name: String,
    pub author: String,
    pub chapter_count: usize,
    pub source: StorySource,
    pub completed: Completed,
}

pub struct FindChapter<'a> {
    pub chapter: &'a Content,
    pub parent: Option<&'a Content>,
}

#[derive(Debug, Clone)]
pub enum Content {
    Section(Section),
    Chapter(Chapter),
}

impl Content {
    pub fn id(&self) -> &str {
        match self {
            Self::Chapter(c) => &c.id,
            Self::Section(s) => &s.id,
        }
    }

    pub fn find_child(&self, id: &str) -> Option<FindChapter> {
        match self {
            Self::Chapter(_) => None,
            Self::Section(s) => s.chapters.iter().find_map(|con| {
                if con.id() == id {
                    Some(FindChapter {
                        chapter: con,
                        parent: Some(self),
                    })
                } else if let Content::Section(_) = con {
                    con.find_child(&id)
                } else {
                    None
                }
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub chapters: Vec<Content>,
    pub url: Option<String>,
    pub author: Option<Author>,
}

impl Section {
    pub fn num_chapters(&self) -> usize {
        self.chapters.iter().fold(0, |acc, sec| match sec {
            Content::Section(inner) => acc + inner.num_chapters(),
            Content::Chapter(_) => acc + 1,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub text: ChapterText,
    pub url: String,
    pub date_posted: DateTime<FixedOffset>,
    pub author: Option<Author>,
}

impl Chapter {
    pub fn chapter_id(&self) -> String {
        match self.id.rfind(":") {
            Some(idx) => self.id[idx + 1..].to_string(),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChapterText {
    Hydrated(String),
    Dehydrated,
}

impl ChapterText {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Hydrated(s) => s,
            Self::Dehydrated => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub id: String,
}

impl Author {
    pub fn new<F: Into<String>>(name: F, id: F) -> Author {
        Author {
            name: name.into(),
            id: id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthorList {
    authors: Vec<Author>,
}

impl AuthorList {
    pub fn new(author: Author) -> AuthorList {
        let mut authors = Vec::with_capacity(1);
        authors.push(author);
        AuthorList { authors }
    }

    pub fn from_list<T: Into<Vec<Author>>>(authors: T) -> AuthorList {
        let authors = authors.into();
        assert!(authors.len() > 0); // TODO: This will panic if 0 authors are passed in
        AuthorList { authors }
    }

    pub fn authors(&self) -> &Vec<Author> {
        &self.authors
    }

    pub fn authors_mut(&mut self) -> &mut Vec<Author> {
        &mut self.authors
    }

    pub fn len(&self) -> usize {
        self.authors.len()
    }
}

#[derive(Debug, Clone)]
pub enum StorySource {
    AO3(String),
    Katalepsis,
    RoyalRoad(String),
    SpaceBattles(String),
    SufficientVelocity(String),
}

pub static SOURCES_LIST: [&str; 5] = [
    "Archive of Our Own: https://archiveofourown.org/works/<id>",
    "Katalepsis: https://katalepsis.net",
    "RoyalRoad: https://www.royalroad.com/fiction/<id>",
    "SpaceBattles: https://forums.spacebattles.com/threads/thread_name.<id>",
    "SufficientVelocity: https://forums.sufficientvelocity.com/threads/thread_name.<id>",
    // "XenForo: https://<site>/threads/thread_name.<id>",
];

static REGEXES: OnceCell<Vec<(&'static str, Regex)>> = OnceCell::new();
#[rustfmt::skip]
fn init_regexes() -> Vec<(&'static str, Regex)> {
    vec![
        ("ao3", r"^https://archiveofourown.org/works/(?P<id>\d+)/?.*"),
        ("ffnet", r"^https?://(?:www)?\.fanfiction\.net/s/(?P<id>\d+)/?.*"),
        ("katalepsis", r"^https?://katalepsis\.net/?.*"),
        ("rr", r"^https?://(?:www)?\.royalroad\.com/fiction/(?P<id>\d+)/?.*"),
        ("sb", r"^https?://forums\.spacebattles\.com/threads/([^.]+\.)?(?P<id>\d+)/?.*"),
        ("sv", r"^https?://forums\.sufficientvelocity\.com/threads/([^.]+\.)?(?P<id>\d+)/?.*"),
    ]
    .into_iter()
    .map(|(src, reg_src)| (src, Regex::new(reg_src).unwrap()))
    .collect()
}

impl StorySource {
    pub fn from_url(url: &str) -> Result<StorySource> {
        let regex_map = REGEXES.get_or_init(init_regexes);
        match regex_map.iter().find(|(_, regex)| regex.is_match(url)) {
            Some((name, regex)) => {
                let id = regex.captures(url).unwrap().name("id");
                Ok(match *name {
                    "ao3" => Self::AO3(
                        id.ok_or(ArchiveError::NoIdInSource(url.to_owned(), name.to_string()))?
                            .as_str()
                            .to_owned(),
                    ),
                    "katalepsis" => Self::Katalepsis,
                    "rr" => Self::RoyalRoad(
                        id.ok_or(ArchiveError::NoIdInSource(url.to_owned(), name.to_string()))?
                            .as_str()
                            .to_owned(),
                    ),
                    "sb" => Self::SpaceBattles(
                        id.ok_or(ArchiveError::NoIdInSource(url.to_owned(), name.to_string()))?
                            .as_str()
                            .to_owned(),
                    ),
                    "sv" => Self::SufficientVelocity(
                        id.ok_or(ArchiveError::NoIdInSource(url.to_owned(), name.to_string()))?
                            .as_str()
                            .to_owned(),
                    ),
                    _ => panic!("URL matched source {name}, which has not been fully implemented"),
                })
            }
            None => Err(ArchiveError::BadSource(url.to_owned())),
        }
    }

    pub fn to_id(&self) -> String {
        match self {
            Self::AO3(id) => format!("{}:{}", self.prefix(), id),
            Self::Katalepsis => self.prefix().to_owned(),
            Self::RoyalRoad(id) => format!("{}:{}", self.prefix(), id),
            Self::SpaceBattles(id) => format!("{}:{}", self.prefix(), id),
            Self::SufficientVelocity(id) => format!("{}:{}", self.prefix(), id),
        }
    }

    #[inline(always)]
    pub fn prefix(&self) -> &str {
        match self {
            Self::AO3(_) => "ao3",
            Self::Katalepsis => "katalepsis",
            Self::RoyalRoad(_) => "rr",
            Self::SpaceBattles(_) => "sb",
            Self::SufficientVelocity(_) => "sv",
        }
    }

    pub fn to_url(&self) -> String {
        match self {
            Self::AO3(id) => {
                format!("https://archiveofourown.org/works/{}", id)
            }
            Self::Katalepsis => "https://katalepsis.net".to_owned(),
            Self::RoyalRoad(id) => format!("https://www.royalroad.com/fiction/{}", id),
            Self::SpaceBattles(id) => format!("https://forums.spacebattles.com/threads/{}", id),
            Self::SufficientVelocity(id) => {
                format!("https://forums.sufficientvelocity.com/threads/{}", id)
            }
        }
    }

    pub fn to_base_url(&self) -> String {
        let url = self.to_url();
        let start = url.find("://").map(|pos| pos + 3).unwrap_or(0);
        let end = url[start..].find("/").unwrap_or(url[start..].len());
        url[0..end + start].to_owned()
    }

    pub fn parser(&self) -> Box<dyn Parser> {
        match self {
            Self::AO3(_) => Box::new(AO3Parser {}),
            Self::Katalepsis => Box::new(KatalepsisParser {}),
            Self::RoyalRoad(_) => Box::new(RoyalRoadParser {}),
            Self::SpaceBattles(_) => Box::new(XenforoParser {}),
            Self::SufficientVelocity(_) => Box::new(XenforoParser {}),
        }
    }
}
