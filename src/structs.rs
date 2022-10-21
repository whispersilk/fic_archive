use chrono::{DateTime, FixedOffset};
use once_cell::sync::OnceCell;
use regex::{Match, Regex};

use crate::error::ArchiveError;
use crate::parser::{
    ao3::AO3Parser, katalepsis::KatalepsisParser, royalroad::RoyalRoadParser, Parser,
};

#[derive(Debug, Clone)]
pub struct Story {
    pub name: String,
    pub author: Author,
    pub description: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
    pub chapters: Vec<Content>,
    pub source: StorySource,
}

impl Story {
    pub fn num_chapters(&self) -> usize {
        self.chapters.iter().fold(0, |acc, con| match con {
            Content::Section(sec) => acc + sec.num_chapters(),
            Content::Chapter(_) => acc + 1,
        })
    }

    pub fn find_chapter(&self, id: String) -> Option<FindChapter> {
        let mut found = None;
        for content in self.chapters.iter() {
            if found.is_some() {
                break;
            } else if content.id() == id.as_str() {
                found = Some(FindChapter {
                    chapter: content,
                    parent: None,
                });
            } else if let Content::Section(s) = content {
                found = s.find_chapter(&id).map(|f| FindChapter {
                    chapter: f.chapter,
                    parent: Some(content),
                });
            }
        }
        found
    }
}

#[derive(Debug, Clone)]
pub struct ListedStory {
    pub name: String,
    pub author: String,
    pub chapter_count: usize,
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
}

#[derive(Debug, Clone)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub chapters: Vec<Content>,
    pub url: Option<String>,
}

impl Section {
    pub fn num_chapters(&self) -> usize {
        self.chapters.iter().fold(0, |acc, sec| match sec {
            Content::Section(inner) => acc + inner.num_chapters(),
            Content::Chapter(_) => acc + 1,
        })
    }

    pub fn find_chapter(&self, id: &String) -> Option<FindChapter> {
        let mut found = None;
        for content in self.chapters.iter() {
            if found.is_some() {
                break;
            } else if content.id() == id.as_str() {
                found = Some(FindChapter {
                    chapter: content,
                    parent: None,
                });
            } else if let Content::Section(s) = content {
                found = s.find_chapter(id).map(|f| FindChapter {
                    chapter: f.chapter,
                    parent: Some(content),
                });
            }
        }
        found
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

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum TextFormat {
    Html,
    Markdown,
}

#[derive(Debug, Clone)]
pub enum StorySource {
    AO3(String),
    Katalepsis,
    RoyalRoad(String),
}

pub static SOURCES_LIST: [&str; 3] = [
    "Archive of Our Own: https://archiveofourown.org/works/<id>",
    "Katalepsis: https://katalepsis.net",
    "RoyalRoad: https://www.royalroad.com/fiction/<id>",
];

static REGEXES: OnceCell<Vec<(&'static str, Regex)>> = OnceCell::new();

impl StorySource {
    pub fn from_url(url: &str) -> Result<StorySource, ArchiveError> {
        let regex_map = REGEXES.get_or_init(|| {
            vec![
                ("ao3", r"^https://archiveofourown.org/works/(?P<id>\d+)/?.*"),
                (
                    "ffnet",
                    r"^https?://(?:www)?\.fanfiction\.net/s/(?P<id>\d+)/?.*",
                ),
                ("katalepsis", r"^https?://katalepsis\.net/?.*"),
                (
                    "rr",
                    r"^https?://(?:www)?\.royalroad\.com/fiction/(?P<id>\d+)/?.*",
                ),
            ]
            .into_iter()
            .map(|(src, reg_src)| (src, Regex::new(reg_src).unwrap()))
            .collect()
        });
        let maybe_match = regex_map.iter().find(|(_, regex)| regex.is_match(url));
        match maybe_match {
            Some((name, regex)) => {
                let id = regex.captures(url).unwrap().name("id");
                let maybe_error = &format!(
                    "Url {url} maps to source {name} and must contain a story ID, but does not"
                );
                Ok(match *name {
                    "ao3" => Self::AO3(require_story_source_id(id, maybe_error)),
                    "katalepsis" => Self::Katalepsis,
                    "rr" => Self::RoyalRoad(require_story_source_id(id, maybe_error)),
                    _ => panic!("URL matched source {name}, which has not been fully implemented"),
                })
            }
            None => Err(ArchiveError::BadSource(url.to_owned())),
        }
    }

    pub fn to_id(&self) -> String {
        match self {
            Self::AO3(ref id) => format!("ao3:{}", id),
            Self::Katalepsis => "katalepsis".to_owned(),
            Self::RoyalRoad(ref id) => format!("rr:{}", id),
        }
    }

    pub fn to_url(&self) -> String {
        match self {
            Self::AO3(id) => {
                format!("https://archiveofourown.org/works/{}", id)
            }
            Self::Katalepsis => "https://katalepsis.net".to_owned(),
            Self::RoyalRoad(id) => format!("https://www.royalroad.com/fiction/{}", id),
        }
    }

    pub fn parser(&self) -> Box<dyn Parser> {
        match self {
            Self::AO3(_) => Box::new(AO3Parser {}),
            Self::Katalepsis => Box::new(KatalepsisParser {}),
            Self::RoyalRoad(_) => Box::new(RoyalRoadParser {}),
        }
    }
}

fn require_story_source_id(id_match: Option<Match>, errormsg: &str) -> String {
    id_match.expect(errormsg).as_str().to_owned()
}

#[derive(Clone, Debug)]
pub struct StoryBase {
    pub title: String,
    pub author: Author,
    pub chapter_links: Vec<ChapterLink>,
}

#[derive(Clone, Debug)]
pub struct ChapterLink {
    pub url: String,
    pub title: String,
}
