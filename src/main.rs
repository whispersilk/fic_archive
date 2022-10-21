use clap::Parser;
use rusqlite::Connection;

use std::collections::HashSet;

use self::args::{Args, Commands};
use self::error::ArchiveError;
use self::structs::{Content, StorySource, SOURCES_LIST};

mod args;
mod error;
mod parser;
mod sql;
mod structs;

#[tokio::main]
async fn main() -> Result<(), ArchiveError> {
    let args = Args::parse();
    let conn = Connection::open("/home/daniel/Documents/Code/fic_archive/test_db.db")?;

    match args.command {
        Commands::Add { story } => add_story(story, &conn).await?,
        Commands::Update {
            story,
            force_refresh,
        } => update_archive(story, force_refresh, &conn).await?,
        Commands::Delete { search } => delete_story(search, &conn).await?,
        Commands::Export { .. } => {
            todo!()
        }
        Commands::List { .. } => list_stories(&conn).await?,
        Commands::ListSources => {
            let len =
                SOURCES_LIST.into_iter().fold(0, |acc, i| acc + i.len()) + SOURCES_LIST.len() - 1;
            println!(
                "{}",
                SOURCES_LIST
                    .into_iter()
                    .fold(String::with_capacity(len), |mut acc, i| {
                        acc.push_str(i);
                        if acc.len() < acc.capacity() {
                            acc.push('\n');
                        }
                        acc
                    })
            );
        }
    }
    // let source = StorySource::from_url("https://www.royalroad.com/fiction/39408/beware-of-chicken");
    // let source = StorySource::from_url("https://www.fanfiction.net/s/3676590");
    // let source = StorySource::from_url("https://katalepsis.net");
    // let source = StorySource::from_url("https://www.royalroad.com/fiction/59450/bioshifter");
    // let source = StorySource::from_url("https://www.royalroad.com/fiction/40373/vigor-mortis");
    // let source = StorySource::from_url("https://archiveofourown.org/works/35394595");

    // let existing_story = sql::get_story_by_id(&conn, &source.to_id())?;

    // let parser: &dyn Parser =  match source {
    //     StorySource::AO3(_) => &AO3Parser {},
    //     StorySource::Katalepsis => &KatalepsisParser {},
    //     StorySource::RoyalRoad(_) => &RoyalRoadParser {},
    // };
    // let story = parser.get_story(&runtime, &TextFormat::Markdown, source)?;

    // if existing_story.is_none() {
    //     sql::save_story(&conn, &story)?;
    //     println!("Saved story!");
    // } else {
    //     println!("Not saving story because it already exists!");
    // }

    Ok(())
}

async fn add_story(story: String, conn: &Connection) -> Result<(), ArchiveError> {
    let source = StorySource::from_url(story.as_str())?;
    if sql::story_exists_with_id(conn, story.as_str())? {
        println!("Story already exists in the archive. Updating...");
        update_archive(Some(story), false, conn).await
    } else {
        let parser = source.parser();
        let story = parser.get_story(source).await?;
        sql::save_story(conn, &story)?;
        println!("Saved {} ({} chapters)", story.name, story.num_chapters());
        Ok(())
    }
}

async fn update_archive(
    story: Option<String>,
    force_refresh: bool,
    conn: &Connection,
) -> Result<(), ArchiveError> {
    if story.is_some() {
        let url = story.unwrap();
        let source = StorySource::from_url(url.as_str())?;
        let result = update_story(source, force_refresh, conn).await?;
        println!("Updated story at {} with {} new chapters.", url, result);
        Ok(())
    } else {
        todo!()
    }
}

async fn update_story(
    source: StorySource,
    force_refresh: bool,
    conn: &Connection,
) -> Result<usize, ArchiveError> {
    let existing_story = sql::get_story_by_id(conn, source.to_id().as_str())?
        .ok_or_else(|| ArchiveError::StoryNotExists(source.to_url()))?;
    let parser = source.parser();
    let client = parser.get_client();
    let new_skeleton = parser.get_skeleton(&client, source).await?;

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
        let new_story = parser.fill_skeleton(&client, new_skeleton).await?;
        for chapter in new_chapters.into_iter() {
            match new_story.find_chapter(chapter) {
                Some(found) => {
                    sql::save_content(
                        conn,
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

fn flatten_content(set: &mut HashSet<String>, content: &Content) {
    set.insert(content.id().to_owned());
    if let Content::Section(s) = content {
        for chapter in s.chapters.iter() {
            flatten_content(set, chapter);
        }
    }
}

async fn delete_story(search: String, conn: &Connection) -> Result<(), ArchiveError> {
    let matches = sql::fuzzy_get_story(conn, search.as_str())?;
    match matches.len() {
        0 => println!("No matching stories found. Please try another search."),
        // 1 => sql::delete_story_by_id(matches[0])?,
        _ => todo!(),
    }
    Ok(())
}

async fn list_stories(conn: &Connection) -> Result<(), ArchiveError> {
    let stories = sql::get_all_stories(conn)?;
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
