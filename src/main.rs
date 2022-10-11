use rusqlite::Connection;
use tokio::runtime;

use self::error::ArchiveError;
use self::parser::{
    Parser,
    katalepsis::KatalepsisParser
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
    let source = StorySource::from_url("https://www.royalroad.com/fiction/39408/beware-of-chicken");
    let source = StorySource::from_url("https://www.fanfiction.net/s/3676590");
    let source = StorySource::from_url("https://katalepsis.net");

    let existing_story = sql::get_story_by_id(&conn, &source.to_id())?;
    println!(
        "Existing story is: {}",
        if existing_story.is_none() {
            "NONE"
        } else {
            "SOME"
        }
    );

    let story = match source {
        StorySource::FFNet(_) => parser::ffnet::get_story(&runtime, TextFormat::Markdown, source)?,
        StorySource::Katalepsis => {
            let parser = KatalepsisParser {};
            parser.get_story(&runtime, &TextFormat::Markdown, source)?
        },
        StorySource::RoyalRoad(_) => {
            parser::royalroad::get_story(&runtime, TextFormat::Markdown, source)?
        }
    };

    if existing_story.is_none() {
        sql::save_story(&conn, &story)?;
        println!("Saved story!");
    } else {
        println!("Not saving story because it already exists!");
    }

    // let mut in_order = Vec::new();
    // story.chapters
    //     .into_iter()
    //     .for_each(|s| {
    //         if let Content::Section { name, chapters, .. } = s {
    //             in_order.push(name.clone());
    //             chapters.iter().for_each(|c| if let Content::Chapter { name, .. } = c { in_order.push(format!("    {}", name)) } else {  });
    //         } else { unreachable!() }
    //     });
    // for i in 0..in_order.len() {
    //     println!("{}", in_order[i]);
    // }

    // if let Content::Section { chapters, .. } = &story.chapters[0] {
    //     if let Content::Chapter { text, .. } = &chapters[0] {
    //         println!("{}", text);
    //     }
    // }

    // if let Content::Chapter { text, .. } = &story.chapters[0] {
    //     println!("{}", text);
    // }

    /*for chapter in story.chapters.iter() {
        match chapter {
            Content::Section {
                name,
                description: _,
                chapters,
                url: _,
            } => println!("{} ({} chapters)", name, chapters.len()),
            _ => (),
        }
    }*/

    Ok(())
}
