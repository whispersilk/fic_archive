use tokio::io::{stdout, AsyncWriteExt as _};
use tokio::runtime;

use self::error::ArchiveError;
use self::structs::Content;

mod error;
mod parser;
mod structs;

fn main() -> Result<(), ArchiveError> {
    let runtime = runtime::Runtime::new()?;
    let story = parser::katalepsis::get_story(&runtime)?;
    println!("{}", story.chapters.len());
    for chapter in story.chapters.iter() {
        match chapter {
            Content::Chapter {
                name,
                description: _,
                text: _,
                url: _,
                date_posted: _,
            } => println!("{}", name),
            Content::Section {
                name,
                description: _,
                chapters,
                url: _,
            } => println!("{} ({} chapters)", name, chapters.len()),
        }
    }

    // stdout().write_all(format!("{}\n", chapters.next().unwrap().await.1).as_bytes()).await?;
    /*for chapter in chapters {
        let chapter = chapter.await;
        stdout().write(format!("{}\n\"{}\"\n\n", chapter.0, chapter.1).as_bytes()).await?;
    }*/
    //for link in toc.iter() {
    //  let href = link.attr("href").unwrap();
    //
    //stdout().write(format!("{} {}\n", link.text(), link.attr("href").unwrap()).as_bytes()).await?;
    //}
    Ok(())
}
