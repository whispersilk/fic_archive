use chrono::{DateTime, FixedOffset};
use std::convert::Into;

use crate::error::ArchiveError;

#[derive(Clone, Default)]
pub struct Story {
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
    pub chapters: Vec<Content>,
}

#[derive(Clone)]
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

#[derive(Clone, Default)]
pub struct StoryBuilder {
    name: Option<String>,
    description: Option<String>,
    url: Option<String>,
    tags: Option<Vec<String>>,
    chapters: Option<Vec<Content>>,
}

impl StoryBuilder {
    pub fn name<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.name = Some(value.into());
        new
    }
    pub fn description<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.description = Some(value.into());
        new
    }
    pub fn url<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.url = Some(value.into());
        new
    }
    pub fn tags<T: Into<Vec<String>>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.tags = Some(value.into());
        new
    }
    pub fn chapters<T: Into<Vec<Content>>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.chapters = Some(value.into());
        new
    }
    pub fn build(&self) -> Result<Story, ArchiveError> {
        Ok(Story {
            name: self
                .name
                .as_ref()
                .ok_or(ArchiveError::StoryBuild(
                    "No name provided before building Story",
                ))?
                .clone(),
            description: self.description.clone(),
            url: self
                .url
                .as_ref()
                .ok_or(ArchiveError::StoryBuild(
                    "No description provided before building Story",
                ))?
                .clone(),
            tags: self.tags.as_ref().unwrap_or(&Vec::new()).clone(),
            chapters: self.chapters.as_ref().unwrap_or(&Vec::new()).clone(),
        })
    }
}

#[derive(Clone, Default)]
pub struct SectionBuilder {
    name: Option<String>,
    description: Option<String>,
    chapters: Option<Vec<Content>>,
    url: Option<String>,
}

impl SectionBuilder {
    pub fn name<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.name = Some(value.into());
        new
    }
    pub fn description<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.description = Some(value.into());
        new
    }
    pub fn chapters<T: Into<Vec<Content>>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.chapters = Some(value.into());
        new
    }
    pub fn url<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.url = Some(value.into());
        new
    }
    pub fn build(&self) -> Result<Content, ArchiveError> {
        Ok(Content::Section {
            name: self
                .name
                .as_ref()
                .ok_or(ArchiveError::ContentBuild(
                    "No name provided before building Content::Section",
                ))?
                .clone(),
            description: self.description.clone(),
            chapters: if let Some(chaps) = &self.chapters {
                chaps.clone()
            } else {
                Vec::new()
            },
            url: self.url.clone(),
        })
    }
}

#[derive(Clone, Default)]
pub struct ChapterBuilder {
    name: Option<String>,
    description: Option<String>,
    text: Option<String>,
    url: Option<String>,
    date_posted: Option<DateTime<FixedOffset>>,
}

impl ChapterBuilder {
    pub fn name<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.name = Some(value.into());
        new
    }
    pub fn description<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.description = Some(value.into());
        new
    }
    pub fn text<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.text = Some(value.into());
        new
    }
    pub fn url<T: Into<String>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.url = Some(value.into());
        new
    }
    pub fn date_posted<T: Into<DateTime<FixedOffset>>>(&mut self, value: T) -> &mut Self {
        let mut new = self;
        new.date_posted = Some(value.into());
        new
    }
    pub fn build(&self) -> Result<Content, ArchiveError> {
        Ok(Content::Chapter {
            name: self
                .name
                .as_ref()
                .ok_or(ArchiveError::ContentBuild(
                    "No name provided before building Content::Chapter",
                ))?
                .clone(),
            description: self.description.clone(),
            text: self
                .text
                .as_ref()
                .ok_or(ArchiveError::ContentBuild(
                    "No text provided before building Content::Chapter",
                ))?
                .clone(),
            url: self
                .url
                .as_ref()
                .ok_or(ArchiveError::ContentBuild(
                    "No url provided before building Content::Chapter",
                ))?
                .clone(),
            date_posted: self
                .date_posted
                .ok_or(ArchiveError::ContentBuild(
                    "No date_posted provided before building Content::Chapter",
                ))?
                .clone(),
        })
    }
}
