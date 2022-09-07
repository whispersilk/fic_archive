use tokio::runtime;

use self::error::ArchiveError;
use self::structs::{Content, StorySource, TextFormat};

mod error;
mod parser;
mod sql;
mod structs;

fn main() -> Result<(), ArchiveError> {
    let runtime = runtime::Runtime::new()?;

    let source = StorySource::from_url("https://www.royalroad.com/fiction/39408/beware-of-chicken");
    //let source = StorySource::from_url("https://katalepsis.net");
    let story = match source {
        StorySource::Katalepsis => parser::katalepsis::get_story(&runtime, TextFormat::Markdown)?,
        StorySource::RoyalRoad(_) => {
            parser::royalroad::get_story(&runtime, TextFormat::Markdown, source)?
        }
    };

    if let Content::Section { chapters, .. } = &story.chapters[0] {
        if let Content::Chapter { text, .. } = &chapters[0] {
            println!("{}", text);
        }
    }

    if let Content::Chapter { text, .. } = &story.chapters[0] {
        println!("{}", text);
    }

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
