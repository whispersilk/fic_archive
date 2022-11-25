use clap::Parser;
use futures::future::join_all;
use std::collections::HashSet;

use self::args::{Args, Commands::*};
use self::error::ArchiveError;
use self::sql::Database;
use self::structs::{Content, StorySource, SOURCES_LIST};
use self::tui::start_tui;

mod args;
mod client;
mod error;
mod parser;
mod sql;
mod structs;
mod tui;

pub type Result<T> = std::result::Result<T, ArchiveError>;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let db = Database::new(&args.db)?;

    match args.command {
        Some(sub) => match sub {
            Add { stories } => add_stories(stories, &db).await?,
            Update {
                story,
                force_refresh,
            } => {
                update_archive(
                    match story {
                        Some(s) => Some(StorySource::from_url(&s)?),
                        None => None,
                    },
                    force_refresh,
                    &db,
                )
                .await?
            }
            Delete { search } => delete_story(search, &db).await?,
            Export { .. } => {
                todo!()
            }
            List { .. } => list_stories(&db).await?,
            ListSources => println!(
                "{}",
                SOURCES_LIST.into_iter().rev().enumerate().rev().fold(
                    String::new(),
                    |mut acc, (idx, source)| {
                        acc.push_str(source);
                        if idx > 0 {
                            acc.push('\n');
                        }
                        acc
                    }
                )
            ),
        },
        None => start_tui(args).await?,
    }

    Ok(())
}

async fn add_stories(stories: Vec<String>, db: &Database) -> Result<()> {
    let mut errors: Vec<ArchiveError> = Vec::new();
    for story in stories.iter() {
        match StorySource::from_url(&story) {
            Ok(source) => match add_story(source, db).await {
                Ok(_) => (),
                Err(err) => errors.push(err),
            },
            Err(err) => errors.push(err),
        };
    }
    errors.into_iter().next().map(|e| Err(e)).unwrap_or(Ok(()))
}

async fn add_story(source: StorySource, db: &Database) -> Result<()> {
    let exists = db.story_exists_with_id(&source.to_id())?;
    let url = source.to_url();
    if exists {
        let new_chapters = update_story(source, false, db).await?;
        println!(
            "Updated story at {} with {} new chapters.",
            url, new_chapters
        );
    } else {
        let story = source.parser().get_story(source).await?;
        db.save_story(&story)?;
        println!(
            "Added story {} ({} chapter{})",
            story.name,
            story.num_chapters(),
            if story.num_chapters() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

async fn update_archive(
    story: Option<StorySource>,
    force_refresh: bool,
    db: &Database,
) -> Result<()> {
    match story {
        Some(source) => {
            let url = source.to_url();
            let result = update_story(source, force_refresh, db).await?;
            println!(
                "{}pdated story at {} with {} new chapters.",
                if force_refresh { "Force-u" } else { "U" },
                url,
                result
            );
            Ok(())
        }
        None => {
            let stories = db.get_all_stories()?;
            let story_count = stories.len();
            let (new_chaps, failed) = join_all(
                stories
                    .into_iter()
                    .map(|s| update_story(s.source, force_refresh, db)),
            )
            .await
            .into_iter()
            .fold((0, 0), |acc, x| match x {
                Ok(num) => (acc.0 + num, acc.1),
                Err(_) => (acc.0, acc.1 + 1),
            });
            println!(
                "{}pdated archive. Got {} new chapters from {} stories. Failed to update {} stories.",
                if force_refresh { "Force-u" } else { "U" },
                new_chaps,
                story_count - failed,
                failed,
            );
            Ok(())
        }
    }
}

async fn update_story(source: StorySource, force_refresh: bool, db: &Database) -> Result<usize> {
    let parser = source.parser();
    if force_refresh {
        let story = parser.get_story(source).await?;
        db.save_story(&story)?;
        Ok(story.num_chapters())
    } else {
        let existing_story = db
            .get_story_by_id(source.to_id().as_str())?
            .ok_or_else(|| ArchiveError::StoryNotExists(source.to_url()))?;
        let new_skeleton = parser.get_skeleton(source).await?;

        // Get a list of existing chapters and a list of fetched chapters, then filter to only fetched chapters that aren't saved.
        let mut existing_chapters: HashSet<String> =
            HashSet::with_capacity(existing_story.chapters.len());
        existing_story
            .chapters
            .iter()
            .for_each(|chap| flatten_content(&mut existing_chapters, chap));
        let mut new_chapters: HashSet<String> = HashSet::with_capacity(new_skeleton.chapters.len());
        new_skeleton
            .chapters
            .iter()
            .for_each(|chap| flatten_content(&mut new_chapters, chap));
        let new_chapters: Vec<String> = new_chapters
            .into_iter()
            .filter(|c| !existing_chapters.contains(c))
            .collect();

        // If there are any new chapters, fetch the story and save them.
        let mut added_chapters = 0;
        if !new_chapters.is_empty() {
            let new_story = parser.fill_skeleton(new_skeleton).await?;
            for chapter in new_chapters.into_iter() {
                match new_story.find_chapter(chapter) {
                    Some(found) => {
                        db.save_content(
                            found.chapter,
                            new_story.source.to_id().as_str(),
                            found.parent.map(|content| content.id()),
                        )?;
                        added_chapters += 1;
                    }
                    None => unreachable!(),
                }
            }
        }
        Ok(added_chapters)
    }
}

fn flatten_content(set: &mut HashSet<String>, content: &Content) {
    set.insert(content.id().to_owned());
    if let Content::Section(s) = content {
        for chapter in s.chapters.iter() {
            flatten_content(set, chapter);
        }
    }
}

async fn delete_story(search: String, db: &Database) -> Result<()> {
    let matches = db.fuzzy_get_story(search.as_str())?;
    match matches.len() {
        0 => println!("No matching stories found. Please try another search."),
        // 1 => sql::delete_story_by_id(matches[0])?,
        1 => println!("Got one story back! Id: {}", matches[0]),
        _ => todo!(),
    }
    Ok(())
}

async fn list_stories(db: &Database) -> Result<()> {
    let stories = db.get_all_stories()?;
    stories.into_iter().for_each(|ls| {
        println!(
            "\"{}\" by {} ({} chapter{})",
            ls.name,
            ls.author,
            ls.chapter_count,
            if ls.chapter_count == 1 { "" } else { "s" }
        )
    });
    Ok(())
}
