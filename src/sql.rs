use chrono::DateTime;
use rayon::prelude::ParallelSliceMut;
use rusqlite::{Connection, Error, Result};

use crate::error::ArchiveError;
use crate::structs::{Author, Chapter, ChapterText, Content, Section, Story, StorySource};

pub fn create_tables(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS authors (
		    id TEXT PRIMARY KEY,
		    name TEXT NOT NULL
		)",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS stories (
		    id TEXT NOT NULL PRIMARY KEY,
		    name TEXT NOT NULL,
		    description TEXT,
		    url TEXT NOT NULL,
		    author_id TEXT NOT NULL REFERENCES authors(id)
		)",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sections (
			id TEXT PRIMARY KEY,
			name TEXT NOT NULL,
			description TEXT,
			url TEXT,
			story_id TEXT NOT NULL REFERENCES stories(id),
			parent_id TEXT
		)",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS chapters (
			id TEXT PRIMARY KEY,
			name TEXT NOT NULL,
			description TEXT,
			text TEXT NOT NULL,
			url TEXT NOT NULL,
			date_posted TEXT NOT NULL,
			story_id TEXT NOT NULL REFERENCES stories(id),
			section_id TEXT REFERENCES sections(id)
		)",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tag_uses (
            tag_id TEXT NOT NULL REFERENCES tags(id),
            story_id TEXT NOT NULL REFERENCES stories(id)
        )",
        (),
    )?;
    Ok(())
}

pub fn get_story_by_id(conn: &Connection, id: &str) -> Result<Option<Story>, ArchiveError> {
    create_tables(conn)?;
    let mut stmt = conn.prepare("SELECT COUNT(1) FROM stories WHERE id = :id")?;
    let story_exists = stmt
        .query_row(&[(":id", id)], |row| match row.get(0) {
            Ok(0) => Ok(None),
            Ok(1) => Ok(Some(())),
            _ => Ok(None),
        });
    let story_exists = story_exists.is_ok() && story_exists.unwrap().is_some();
    if !story_exists {
        Ok(None)
    } else {
        stmt = conn.prepare(
            "SELECT id, name, description, url, parent_id FROM sections WHERE story_id = :story_id",
        )?;
        let mut sections: Vec<(Option<String>, Section)> = stmt
            .query_map(&[(":story_id", id)], |row| {
                let section_id = row.get::<usize, String>(4)?;
                let section_id = match section_id.as_str() {
                    "NULL" => None,
                    _ => Some(section_id),
                };
                Ok((
                    section_id,
                    Section {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: {
                            let desc: String = row.get(2)?;
                            match desc.as_str() {
                                "NULL" => None,
                                _ => Some(desc),
                            }
                        },
                        chapters: Vec::new(),
                        url: {
                            let url: String = row.get(2)?;
                            match url.as_str() {
                                "NULL" => None,
                                _ => Some(url),
                            }
                        },
                    },
                ))
            })?
            .map(|sec| sec.unwrap())
            .collect();

        stmt = conn.prepare("SELECT id, name, description, text, url, date_posted, section_id FROM chapters WHERE story_id = :story_id")?;
        let mut chapters: Vec<(Option<String>, Chapter)> = stmt
            .query_map(&[(":story_id", id)], |row| {
                let section_id = row.get::<usize, String>(6)?;
                let section_id = match section_id.as_str() {
                    "NULL" => None,
                    _ => Some(section_id),
                };

                Ok((
                    section_id,
                    Chapter {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: {
                            let desc: String = row.get(2)?;
                            match desc.as_str() {
                                "NULL" => None,
                                _ => Some(desc),
                            }
                        },
                        text: ChapterText::Hydrated(row.get(3)?),
                        url: row.get(4)?,
                        date_posted: DateTime::parse_from_rfc3339(
                            row.get::<usize, String>(5)?.as_str(),
                        )
                        .unwrap_or_else(|_| {
                            panic!(
                                "Chapter posted-on date ({:?}) did not conform to rfc3339",
                                row.get::<usize, String>(5)
                            )
                        }),
                    },
                ))
            })?
            .map(|chap| chap.unwrap())
            .collect();

        if !chapters.is_empty() {
            for idx in (0..chapters.len() - 1).rev() {
                if chapters[idx].0.is_some() {
                    let (parent_id, chapter) = chapters.remove(idx);
                    let parent_id = parent_id.unwrap();
                    if let Some((_, section)) = sections.iter_mut().find(|(_, s)| s.id == parent_id)
                    {
                        section.chapters.push(Content::Chapter(chapter));
                    } else {
                        panic!(
                            "Chapter {} has section_id {} that does not match any section",
                            chapter.id, parent_id
                        );
                    }
                }
            }
        }
        if !sections.is_empty() {
            for idx in (0..sections.len() - 1).rev() {
                sections[idx]
                    .1
                    .chapters
                    .par_sort_unstable_by(|a, b| a.id().cmp(b.id()));
                if sections[idx].0.is_some() {
                    let (parent_id, section) = sections.remove(idx);
                    let parent_id = parent_id.unwrap();
                    if let Some((_, parent)) = sections.iter_mut().find(|(_, s)| s.id == parent_id)
                    {
                        parent.chapters.push(Content::Section(section));
                    }
                }
            }
        }
        let mut story_chapters: Vec<Content> = sections
            .into_iter()
            .map(|(_, section)| Content::Section(section))
            .chain(
                chapters
                    .into_iter()
                    .map(|(_, chapter)| Content::Chapter(chapter)),
            )
            .collect();
        story_chapters.par_sort_unstable_by(|a, b| a.id().cmp(b.id()));

        stmt = conn.prepare(
            "SELECT
                tags.name
            FROM tag_uses INNER JOIN tags
            ON tags.id = tag_uses.tag_id
            WHERE tag_uses.story_id = :story_id",
        )?;
        let story_tags: Vec<String> = stmt
            .query_map(&[(":story_id", id)], |row| row.get::<usize, String>(0))?
            .filter(|res| res.is_ok())
            .map(|res| res.unwrap())
            .collect();

        stmt = conn.prepare(
            "SELECT
                stories.name,
                stories.description,
                stories.url,
                stories.author_id,
                authors.name
            FROM stories INNER JOIN authors
            ON stories.author_id = authors.id
            WHERE stories.id = :story_id",
        )?;
        stmt
            .query_row(&[(":story_id", id)], |row| {
                Ok(Story {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    url: row.get(2)?,
                    author: Author {
                        id: row.get(3)?,
                        name: row.get(4)?,
                    },
                    chapters: story_chapters,
                    tags: story_tags,
                    source: StorySource::from_url(row.get::<usize, String>(2)?.as_str()),
                })
            })
            .map(|story| Some(story))
            .map_err(|err| err.into())
    }
}

pub fn save_story(conn: &Connection, story: &Story) -> Result<(), ArchiveError> {
    create_tables(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO authors (id, name) VALUES (?1, ?2)",
        (&story.author.id, &story.author.name),
    )?;
    conn.execute(
        "INSERT INTO stories (id, name, description, url, author_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            &story.source.to_id(),
            &story.name,
            some_or_null(&story.description),
            &story.url,
            &story.author.id,
        ),
    )?;
    for content in story.chapters.iter().as_ref() {
        save_content(conn, content, &story.source.to_id(), None)?;
    }
    for tag in story.tags.iter().as_ref() {
        let tag_id = tag.to_lowercase();
        conn.execute(
            "INSERT OR IGNORE INTO tags (id, name) VALUES (?1, ?2)",
            (&tag_id, &tag),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO tag_uses (tag_id, story_id) VALUES (?1, ?2)",
            (&tag_id, &story.source.to_id()),
        )?;
    }
    Ok(())
}

fn save_content(
    conn: &Connection,
    content: &Content,
    story_id: &str,
    parent_id: Option<&str>,
) -> Result<(), ArchiveError> {
    match content {
        Content::Section(Section {
            id,
            name,
            description,
            chapters,
            url,
        }) => {
            conn.execute("INSERT INTO sections (id, name, description, url, story_id, parent_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				(
					id,
					name,
					some_or_null(description),
					some_or_null(url),
					story_id,
					if parent_id.is_none() { "NULL" } else { parent_id.unwrap() }
				)
			)?;
            for inner in chapters.iter() {
                save_content(conn, inner, story_id, Some(id))?;
            }
        }
        Content::Chapter(Chapter {
            id,
            name,
            description,
            text,
            url,
            date_posted,
        }) => {
            conn.execute("INSERT INTO chapters (id, name, description, text, url, date_posted, story_id, section_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
				(
					id,
					name,
					some_or_null(description),
					text.as_str(),
					url,
					&date_posted.to_rfc3339(),
					story_id,
					if parent_id.is_none() { "NULL" } else { parent_id.unwrap() }
				)
			)?;
        }
    }
    Ok(())
}

#[inline]
fn some_or_null(optstr: &Option<String>) -> &str {
    if optstr.is_none() {
        "NULL"
    } else {
        optstr.as_ref().unwrap()
    }
}
