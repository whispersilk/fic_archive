use chrono::DateTime;
use rayon::prelude::ParallelSliceMut;
use rusqlite::{types::Type, Connection, Error, Result, Row};

use std::ops::{Deref, DerefMut};
use std::sync::Mutex;

use crate::error::ArchiveError;
use crate::structs::{
    Author, Chapter, ChapterText, Content, ListedStory, Section, Story, StorySource,
};

static TABLES_CREATED: once_cell::sync::OnceCell<Mutex<bool>> = once_cell::sync::OnceCell::new();

pub fn create_tables(conn: &Connection) -> Result<(), Error> {
    let mut lock = TABLES_CREATED
        .get_or_init(|| Mutex::new(false))
        .lock()
        .unwrap();
    if !lock.deref() {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS authors (
    		    id TEXT PRIMARY KEY,
    		    name TEXT NOT NULL
    		)",
            (),
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS stories (
    		    id TEXT NOT NULL PRIMARY KEY,
    		    name TEXT NOT NULL,
    		    description TEXT,
    		    url TEXT NOT NULL,
    		    author_id TEXT NOT NULL,
                FOREIGN KEY (author_id) REFERENCES authors(id)
    		)",
            (),
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sections (
    			id TEXT PRIMARY KEY,
    			name TEXT NOT NULL,
    			description TEXT,
    			url TEXT,
    			story_id TEXT NOT NULL,
    			parent_id TEXT,
                FOREIGN KEY (story_id) REFERENCES stories(id) 
    		)",
            (),
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chapters (
    			id TEXT PRIMARY KEY,
    			name TEXT NOT NULL,
    			description TEXT,
    			text TEXT NOT NULL,
    			url TEXT NOT NULL,
    			date_posted TEXT NOT NULL,
    			story_id TEXT NOT NULL,
    			section_id TEXT,
                FOREIGN KEY (story_id) REFERENCES stories(id),
                FOREIGN KEY (section_id) REFERENCES sections(id)
    		)",
            (),
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            )",
            (),
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tag_uses (
                tag_id TEXT NOT NULL,
                story_id TEXT NOT NULL,
                FOREIGN KEY (tag_id) REFERENCES tags(id),
                FOREIGN KEY (story_id) REFERENCES stories(id)
            )",
            (),
        )
        .unwrap();
        *lock.deref_mut() = true;
    }
    Ok(())
}

pub fn get_all_stories(conn: &Connection) -> Result<Vec<ListedStory>, ArchiveError> {
    create_tables(conn).unwrap();
    let mut failed_stories = 0;
    let mut stmt = conn
        .prepare(
            "SELECT
            stories.name,
            authors.name,
            COUNT(chapters.id) AS chapter_count
        FROM stories
            INNER JOIN authors ON stories.author_id = authors.id
            INNER JOIN chapters ON stories.id = chapters.story_id
        GROUP BY stories.id",
        )
        .unwrap();
    let stories: Vec<ListedStory> = stmt
        .query_map([], |row| {
            Ok(ListedStory {
                name: row.get(0).unwrap(),
                author: row.get(1).unwrap(),
                chapter_count: row.get(2).unwrap(),
            })
        })
        .unwrap()
        .filter_map(|listed| match listed {
            Ok(story) => Some(story),
            Err(_) => {
                failed_stories += 1;
                None
            }
        })
        .collect();
    println!(
        "Got {} stories. Failed to get {failed_stories} stories.",
        stories.len()
    );

    Ok(stories)
}

pub fn story_exists_with_id(conn: &Connection, id: &str) -> Result<bool, ArchiveError> {
    create_tables(conn).unwrap();
    let mut stmt = conn
        .prepare("SELECT COUNT(*) FROM stories WHERE id = :id")
        .unwrap();
    let story_exists = stmt
        .query_row(&[(":id", id)], |row| match row.get(0) {
            Ok(0) => Ok(None),
            Ok(1) => Ok(Some(())),
            _ => Ok(None),
        })
        .unwrap_or(None);
    Ok(story_exists.is_some())
}

pub fn fuzzy_get_story(conn: &Connection, search: &str) -> Result<Vec<String>, ArchiveError> {
    create_tables(conn).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT stories.id
        FROM stories INNER JOIN authors ON stories.author_id = authors.id
        WHERE
            stories.name LIKE %:search%
            OR stories.id = :search
            OR authors.name LIKE %:search%",
        )
        .unwrap();
    let matches = stmt
        .query_map(&[(":search", search)], |row| Ok(row.get(0).unwrap()))
        .unwrap()
        .filter_map(|id| id.ok())
        .collect();
    Ok(matches)
}

pub fn get_story_by_id(conn: &Connection, id: &str) -> Result<Option<Story>, ArchiveError> {
    if !story_exists_with_id(conn, id).unwrap() {
        Ok(None)
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, url, parent_id FROM sections WHERE story_id = :story_id",
        ).unwrap();
        let mut sections: Vec<(Option<String>, Section)> = stmt
            .query_map(&[(":story_id", id)], |row| {
                Ok((
                    // ID of parent section, if one exists
                    match is_null(row, 4) {
                        true => None,
                        false => Some(row.get(4)?),
                    },
                    Section {
                        id: row.get(0).unwrap(),
                        name: row.get(1).unwrap(),
                        description: match is_null(row, 2) {
                            true => None,
                            false => Some(row.get(2)?),
                        },
                        chapters: Vec::new(),
                        url: match is_null(row, 3) {
                            true => None,
                            false => Some(row.get(3)?),
                        },
                    },
                ))
            })
            .unwrap()
            .map(|sec| sec.unwrap())
            .collect();

        stmt = conn
            .prepare(
                "SELECT id, name, description, text, url, date_posted, section_id
            FROM chapters
            WHERE story_id = :story_id",
            )
            .unwrap();
        let mut chapters: Vec<(Option<String>, Chapter)> = stmt
            .query_map(&[(":story_id", id)], |row| {
                Ok((
                    // ID of parent section, if one exists
                    match is_null(row, 6) {
                        true => None,
                        false => Some(row.get(6)?),
                    },
                    Chapter {
                        id: row.get(0).unwrap(),
                        name: row.get(1).unwrap(),
                        description: match is_null(row, 2) {
                            true => None,
                            false => Some(row.get(2)?),
                        },
                        text: ChapterText::Hydrated(row.get(3).unwrap()),
                        url: row.get(4).unwrap(),
                        date_posted: DateTime::parse_from_rfc3339(
                            row.get::<usize, String>(5).unwrap().as_str(),
                        )
                        .unwrap_or_else(|_| {
                            panic!(
                                "Chapter posted-on date ({:?}) did not conform to rfc3339",
                                row.get::<usize, String>(5)
                            )
                        }),
                    },
                ))
            })
            .unwrap()
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

        stmt = conn
            .prepare(
                "SELECT tags.name
            FROM tag_uses INNER JOIN tags
            ON tags.id = tag_uses.tag_id
            WHERE tag_uses.story_id = :story_id",
            )
            .unwrap();
        let story_tags: Vec<String> = stmt
            .query_map(&[(":story_id", id)], |row| row.get::<usize, String>(0))
            .unwrap()
            .filter(|res| res.is_ok())
            .map(|res| res.unwrap())
            .collect();

        stmt = conn.prepare(
            "SELECT stories.name, stories.description, stories.url, stories.author_id, authors.name
            FROM stories INNER JOIN authors
            ON stories.author_id = authors.id
            WHERE stories.id = :story_id",
        ).unwrap();
        let mut story = stmt
            .query_row(&[(":story_id", id)], |row| {
                let source = StorySource::from_url(row.get::<usize, String>(2).unwrap().as_str())
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                    .unwrap();
                Ok((
                    row.get::<usize, String>(2).unwrap(),
                    Story {
                        name: row.get(0).unwrap(),
                        description: row.get(1).unwrap(),
                        url: row.get(2).unwrap(),
                        author: Author {
                            id: row.get(3).unwrap(),
                            name: row.get(4).unwrap(),
                        },
                        chapters: story_chapters,
                        tags: story_tags,
                        source,
                    },
                ))
            })
            .map_err(ArchiveError::from)
            .unwrap();
        story.1.source = StorySource::from_url(story.0.as_str()).unwrap();
        Ok(Some(story.1))
    }
}

pub fn save_story(conn: &Connection, story: &Story) -> Result<(), ArchiveError> {
    create_tables(conn).unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO authors (id, name) VALUES (?1, ?2)",
        (&story.author.id, &story.author.name),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO stories (id, name, description, url, author_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            &story.source.to_id(),
            &story.name,
            &story.description,
            &story.url,
            &story.author.id,
        ),
    )
    .unwrap();
    for content in story.chapters.iter().as_ref() {
        save_content(conn, content, &story.source.to_id(), None).unwrap();
    }
    for tag in story.tags.iter().as_ref() {
        let tag_id = tag.to_lowercase();
        conn.execute(
            "INSERT OR IGNORE INTO tags (id, name) VALUES (?1, ?2)",
            (&tag_id, &tag),
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO tag_uses (tag_id, story_id) VALUES (?1, ?2)",
            (&tag_id, &story.source.to_id()),
        )
        .unwrap();
    }
    Ok(())
}

pub fn save_content(
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
					description,
					url,
					story_id,
					parent_id
				)
			).unwrap();
            for inner in chapters.iter() {
                save_content(conn, inner, story_id, Some(id)).unwrap();
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
					description,
					text.as_str(),
					url,
					&date_posted.to_rfc3339(),
					story_id,
					parent_id
				)
			).expect(format!("Failed to add chapter with values\nid: {}\nname: {}\nurl: {}\ndate_posted: {}\nstory_id: {}\nsection_id {}", id, name, url, date_posted.to_rfc3339(), story_id, "NULL").as_str());
        }
    }
    Ok(())
}

fn is_null(row: &Row, column: usize) -> bool {
    matches!(
        row.get::<usize, String>(column),
        Err(Error::InvalidColumnType(_, _, Type::Null))
    )
}
