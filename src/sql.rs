use crate::error::ArchiveError;
use crate::structs::{Chapter, Content, Section, Story};
use chrono::DateTime;
use rusqlite::{Connection, Error, Result};

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
    // We don't yet have a "Tags" table
    Ok(())
}

pub fn get_story_by_id(conn: &Connection, id: &str) -> Result<Option<Story>, ArchiveError> {
    create_tables(conn)?;
    let mut stmt = conn.prepare("SELECT COUNT(1) FROM stories WHERE id = :id")?;
    let story_exists = stmt
        .query_map(&[(":id", id)], |row| match row.get(0) {
            Ok(0) => Ok(None),
            Ok(1) => Ok(Some(())),
            _ => Ok(None),
        })
        .unwrap()
        .next()
        .is_some();
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
        let chapters: Vec<(Option<String>, Chapter)> = stmt
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
                        text: row.get(3)?,
                        url: row.get(4)?,
                        date_posted: DateTime::parse_from_rfc3339({
                            let s: String = row.get(5)?;
                            s.clone().as_str()
                        })
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

        if !sections.is_empty() {
            for chap in chapters
                .into_iter()
                .filter(|chap| chap.0.is_some())
                .map(|chap| (chap.0.unwrap(), chap.1))
            {
                let (parent_id, section) =
                    sections.iter_mut().find(|sec| sec.1.id == chap.0).expect(
                        format!(
                            "Chapter with id {} points to non-existent section with id {}",
                            chap.1.id, chap.0
                        )
                        .as_str(),
                    );
                assert!(matches!(section, Section { .. }));

                //section.chapters.push(chap.1);
            }
        }
        // let chapters =
        Ok(None)
    }
}

pub fn save_story(conn: &Connection, story: &Story) -> Result<(), ArchiveError> {
    create_tables(conn)?;
    conn.execute(
        "INSERT INTO authors (id, name) VALUES (?1, ?2)",
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
					text,
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

/*

Tables:

CREATE TABLE IF NOT EXISTS authors (
    id: VARCHAR(64) PRIMARY KEY,
    name: TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS stories {
    id VARCHAR(64) NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    author_id VARCHAR(64) NOT NULL REFERENCES authors(id),
};

CREATE TABLE IF NOT EXISTS sections {
    id VARCHAR(64),
    story_id
};

CREATE TABLE IF NOT EXISTS chapters {
    id VARCHAR(64) PRIMARY KEY,
    story_id VARCHAR(64) NOT NUMM REFERENCES stories(id),
    section_id VARCHAR(64) REFERENCES sections(id),
    name TEXT
};\

*/
