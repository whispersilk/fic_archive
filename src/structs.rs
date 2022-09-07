use chrono::{DateTime, FixedOffset};
use regex::Regex;

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

#[derive(Debug, Clone)]
pub enum Content {
    Section {
        name: String,
        description: Option<String>,
        chapters: Vec<Content>,
        url: Option<String>,
    },
    Chapter {
        name: String,
        description: Option<String>,
        text: String,
        url: String,
        date_posted: DateTime<FixedOffset>,
    },
}

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct StoryBase {
    pub title: String,
    pub author: Author,
    pub chapter_links: Vec<ChapterLink>,
}

#[derive(Debug, Clone)]
pub struct ChapterLink {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum TextFormat {
    #[allow(dead_code)]
    Html,
    Markdown,
}

#[derive(Debug, Clone)]
pub enum StorySource {
    Katalepsis,
    RoyalRoad(String),
}

static KATALEPSIS_REGEX: (&'static str, once_cell::sync::OnceCell<regex::Regex>) = (
    r"https?://katalepsis\.net/?.*",
    once_cell::sync::OnceCell::new(),
);
static ROYALROAD_REGEX: (&'static str, once_cell::sync::OnceCell<regex::Regex>) = (
    r"https?://(?:www)?\.royalroad\.com/fiction/(\d+)/?\.*",
    once_cell::sync::OnceCell::new(),
);

impl StorySource {
    pub fn from_url(url: &str) -> StorySource {
        if KATALEPSIS_REGEX
            .1
            .get_or_init(|| Regex::new(KATALEPSIS_REGEX.0).unwrap())
            .is_match(url)
        {
            StorySource::Katalepsis
        } else if ROYALROAD_REGEX
            .1
            .get_or_init(|| Regex::new(ROYALROAD_REGEX.0).unwrap())
            .is_match(url)
        {
            let story_id = ROYALROAD_REGEX
                .1
                .get()
                .unwrap()
                .captures(url)
                .unwrap()
                .get(1)
                .expect("Url must contain a story id")
                .as_str()
                .to_owned();
            StorySource::RoyalRoad(story_id)
        } else {
            panic!("URL did not match any available schema.")
        }
    }

    pub fn to_id(&self) -> String {
        match self {
            StorySource::Katalepsis => "katalepsis".to_owned(),
            StorySource::RoyalRoad(ref id) => format!("rr:{}", id),
        }
    }

    pub fn to_url(&self) -> String {
        match self {
            StorySource::Katalepsis => "https://katalepsis.net/table-of-contents".to_owned(),
            StorySource::RoyalRoad(id) => format!("https://www.royalroad.com/fiction/{}", id),
        }
    }
}
