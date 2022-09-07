use html2md::parse_html;
use pandoc::{InputFormat, InputKind, OutputFormat, OutputKind, PandocOutput};

use chrono::DateTime;
use futures::future::join_all;
use reqwest::{Client, Response};
use select::{document::Document, predicate};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    structs::{Author, ChapterLink, Content, Story, StoryBase, StorySource, TextFormat},
};

pub fn get_chapter_list(
    runtime: &Runtime,
    client: &Client,
    story_url: &str,
) -> Result<StoryBase, ArchiveError> {
    let main_page = runtime.block_on(async {
        let intermediate = client.get(story_url).send().await?;
        intermediate.text().await
    })?;

    let document = Document::from_read(main_page.as_bytes())?;
    let pages = document
        .find(predicate::Attr("id", "chapters"))
        .into_selection()
        .find(predicate::Name("tbody"))
        .find(predicate::Name("a"))
        .into_iter()
        .filter(|tag| tag.attr("data-content") == None)
        .map(|link| ChapterLink {
            url: format!(
                "https://www.royalroad.com{}",
                link.attr("href")
                    .expect("A link in the ToC had no href")
                    .to_owned()
            ),
            title: link.text().trim().to_owned(),
        })
        .collect();
    let title = document
        .find(predicate::Class("fic-title"))
        .into_selection()
        .find(predicate::Name("h1"))
        .into_iter()
        .next()
        .expect("Could not find story title")
        .text();
    let author = document
        .find(predicate::Class("fic-title"))
        .into_selection()
        .find(predicate::Attr("property", "author"))
        .children()
        .find(predicate::Name("a"))
        .into_iter()
        .next()
        .expect("Cannot find author name");
    let author = Author {
        name: author.text().trim().to_owned(),
        id: format!(
            "rr:{}",
            author
                .attr("href")
                .expect("Author should have a profile link")
                .replace("/profile/", "")
        ),
    };

    Ok(StoryBase {
        title,
        author,
        chapter_links: pages,
    })
}

pub fn get_story(
    runtime: &Runtime,
    format: TextFormat,
    source: StorySource,
) -> Result<Story, ArchiveError> {
    let client = Client::new();

    let chapter_listing = get_chapter_list(runtime, &client, &source.to_url())?;
    let pages = chapter_listing
        .chapter_links
        .into_iter()
        .map(|chapter| chapter.url)
        .map(|href| client.get(href).send());
    let mut chapter_pages = runtime.block_on(async { join_all(pages).await });
    for idx in 0..chapter_pages.len() {
        let elem = chapter_pages.get(idx).unwrap();
        if elem.is_err() {
            let err = chapter_pages.remove(idx).unwrap_err();
            return Err(ArchiveError::Request(err));
        }
    }
    let mut chapter_urls: Vec<String> = Vec::with_capacity(chapter_pages.len());
    let chapter_pages: Vec<Response> = chapter_pages
        .into_iter()
        .map(|page| page.unwrap())
        .collect();
    for idx in 0..chapter_pages.len() {
        chapter_urls.push(chapter_pages.get(idx).unwrap().url().as_str().to_owned());
    }
    let chapters: Vec<Content> = runtime
        .block_on(async { join_all(chapter_pages.into_iter().map(|page| page.text())).await })
        .into_iter()
        .zip(chapter_urls) // Pair of (text, url)
        .map(|page_plus_url| {
            let document = Document::from_read(page_plus_url.0?.as_bytes())?;
            let title = document
                .find(predicate::Name("div"))
                .into_selection()
                .find(predicate::Class("fic-header"))
                .find(predicate::Name("h1"))
                .into_iter()
                .next()
                .expect("Chapter does not have title")
                .text();
            let body_text: String = document
                .find(predicate::Class("chapter-content"))
                .into_selection()
                .find(predicate::Name("p"))
                .into_iter()
                .map(|elem| match format {
                    TextFormat::Html => elem.html(),
                    TextFormat::Markdown => {
                        let mut pandoc = pandoc::new();
                        pandoc
                            .set_input_format(InputFormat::Html, Vec::new())
                            .set_output_format(OutputFormat::MarkdownStrict, Vec::new())
                            .set_input(InputKind::Pipe(elem.html()))
                            .set_output(OutputKind::Pipe);
                        match pandoc.execute() {
                            Ok(PandocOutput::ToBuffer(text)) => text,
                            _ => parse_html(elem.html().as_str()),
                        }
                    }
                })
                .collect();
            let date_posted = document
                .find(predicate::Class("fa-calendar"))
                .into_selection()
                .parent()
                .find(predicate::Name("time"))
                .into_iter()
                .next()
                .expect("Could not find chapter posted-on date")
                .attr("datetime")
                .expect("Could not find chapter posted-on date attr");
            Ok(Content::Chapter {
                name: title,
                description: None,
                text: body_text,
                url: page_plus_url.1,
                date_posted: DateTime::parse_from_rfc3339(date_posted).unwrap_or_else(|_| {
                    panic!(
                        "Chapter posted-on date ({}) did not conform to rfc3339",
                        date_posted
                    )
                }),
            })
        })
        .map(|c: Result<_, ArchiveError>| c.unwrap())
        .collect();

    Ok(Story {
        name: chapter_listing.title,
        author: chapter_listing.author,
        description: None,
        url: source.to_url(),
        tags: Vec::new(),
        chapters,
        source,
    })
}
