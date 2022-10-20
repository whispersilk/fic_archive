use chrono::{DateTime, FixedOffset};
use once_cell::sync::OnceCell;
use regex::{Match, Regex};

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

#[derive(Debug, Clone)]
pub enum TextFormat {
    #[allow(dead_code)]
    Html,
    Markdown,
}

#[derive(Debug, Clone)]
pub enum StorySource {
    AO3(String),
    FFNet(String),
    Katalepsis,
    RoyalRoad(String),
}

static REGEXES: OnceCell<Vec<(&'static str, Regex)>> = OnceCell::new();

impl StorySource {
    pub fn from_url(url: &str) -> StorySource {
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
        regex_map
            .iter()
            .find(|(_, regex)| regex.is_match(url))
            .map(|(name, regex)| {
                let id = regex.captures(url).unwrap().name("id");
                let maybe_error = &format!(
                    "Url {url} maps to source {name} and must contain a story ID, but does not"
                );
                match *name {
                    "ao3" => StorySource::AO3(require_story_source_id(id, maybe_error)),
                    "ffnet" => StorySource::FFNet(require_story_source_id(id, maybe_error)),
                    "katalepsis" => StorySource::Katalepsis,
                    "rr" => StorySource::RoyalRoad(require_story_source_id(id, maybe_error)),
                    _ => panic!("No way to convert source {name} to a StorySource"),
                }
            })
            .unwrap() // At this point we know we have a Some() - we'd have panicked otherwise
    }

    pub fn to_id(&self) -> String {
        match self {
            StorySource::AO3(ref id) => format!("ao3:{}", id),
            StorySource::FFNet(ref id) => format!("ffnet:{}", id),
            StorySource::Katalepsis => "katalepsis".to_owned(),
            StorySource::RoyalRoad(ref id) => format!("rr:{}", id),
        }
    }

    pub fn to_url(&self) -> String {
        match self {
            StorySource::AO3(id) => {
                format!("https://archiveofourown.org/works/{}", id)
            }
            StorySource::FFNet(id) => format!("https://www.fanfiction.net/s/{}", id),
            StorySource::Katalepsis => "https://katalepsis.net".to_owned(),
            StorySource::RoyalRoad(id) => format!("https://www.royalroad.com/fiction/{}", id),
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
