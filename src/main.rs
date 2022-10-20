use rusqlite::Connection;
use tokio::runtime;

use self::error::ArchiveError;
use self::parser::{
    ao3::AO3Parser, katalepsis::KatalepsisParser, royalroad::RoyalRoadParser, Parser,
};
use self::structs::{StorySource, TextFormat};

mod error;
mod parser;
mod sql;
mod structs;

fn main() -> Result<(), ArchiveError> {
    // Don't default to the number of cores on the machine. We're using threads
    // for I/O so each one will spend a majority of its time waiting.
    rayon::ThreadPoolBuilder::new()
        .num_threads(32)
        .build_global()
        .unwrap();

    let runtime = runtime::Runtime::new()?;
    let conn = Connection::open("/home/daniel/Documents/Code/fic_archive/test_db.db")?;
    // let source = StorySource::from_url("https://www.royalroad.com/fiction/39408/beware-of-chicken");
    // let source = StorySource::from_url("https://www.fanfiction.net/s/3676590");
    // let source = StorySource::from_url("https://katalepsis.net");
    // let source = StorySource::from_url("https://www.royalroad.com/fiction/59450/bioshifter");
    let source = StorySource::from_url("https://www.royalroad.com/fiction/40373/vigor-mortis");
    // let source = StorySource::from_url("https://archiveofourown.org/works/35394595");

    let existing_story = sql::get_story_by_id(&conn, &source.to_id())?;

    let parser: &dyn Parser =  match source {
        StorySource::AO3(_) => &AO3Parser {},
        StorySource::Katalepsis => &KatalepsisParser {},
        StorySource::RoyalRoad(_) => &RoyalRoadParser {},
        // StorySource::FFNet(_) => parser::ffnet::get_story(&runtime, TextFormat::Markdown, source)?,
        StorySource::FFNet(_) => unreachable!(),
    };
    let story = parser.get_story(&runtime, &TextFormat::Markdown, source)?;

    if existing_story.is_none() {
        sql::save_story(&conn, &story)?;
        println!("Saved story!");
    } else {
        println!("Not saving story because it already exists!");
    }

    Ok(())
}
