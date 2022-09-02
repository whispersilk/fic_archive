use chrono::DateTime;
use futures::future::join_all;
use reqwest::{Client, Response};
use select::{
    document::Document,
    predicate::{self, Predicate},
};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    structs::{ChapterBuilder, Content, Story, StoryBuilder},
};

pub fn get_story(runtime: &Runtime) -> Result<Story, ArchiveError> {
    let client = Client::new();

    let response = runtime.block_on(async {
        let intermediate = client
            .get("https://katalepsis.net/table-of-contents/")
            .send()
            .await?;
        intermediate.text().await
    })?;

    let document = Document::from_read(response.as_bytes())?;
    let toc = document
        .find(predicate::Name("div"))
        .into_selection()
        .find(predicate::Class("entry-content"))
        .find(predicate::Name("a"));
    let pages = toc
        .iter()
        //.take(1)
        .map(|link| link.attr("href").expect("A link in the ToC had no href"))
        .map(|href| client.get(href).send());
    let mut chapter_pages = runtime.block_on(async { join_all(pages).await });
    for idx in 0..chapter_pages.len() {
        let elem = chapter_pages.get(idx).unwrap();
        if let Err(_) = elem {
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
    let chapters: Vec<Result<Content, ArchiveError>> = runtime
        .block_on(async { join_all(chapter_pages.into_iter().map(|page| page.text())).await })
        .into_iter()
        .zip(chapter_urls) // Pair of (text, url)
        .map(|page_plus_url| {
            let document = Document::from_read(page_plus_url.0?.as_bytes())?;
            let title = document
                .find(predicate::Class("entry-header").child(predicate::Class("entry-title")))
                .next()
                .expect("Chapter does not have title")
                .text();
            let body_text: String = document
                .find(predicate::Class("entry-content").child(predicate::Class("western")))
                .map(|elem| elem.html())
                .collect();
            //let body_text = parse_html(body_text.as_str());
            let date_posted = document
                .find(predicate::Class("entry-date"))
                .next()
                .expect("Could not find chapter posted-on date")
                .attr("datetime")
                .expect("Could not find chapter posted-on date attr");
            let mut builder: ChapterBuilder = Default::default();
            builder
                .name(title)
                .text(body_text)
                .url(page_plus_url.1)
                .date_posted(
                    DateTime::parse_from_rfc3339(date_posted).expect(
                        format!(
                            "Chapter posted-on date ({}) did not conform to rfc3339",
                            date_posted
                        )
                        .as_str(),
                    ),
                );

            builder.build()
        })
        .collect();

    let mut story_builder: StoryBuilder = Default::default();
    story_builder
        .name("Katalepsis")
        .chapters(
            chapters
                .into_iter()
                .map(|chapter| chapter.unwrap())
                .collect::<Vec<Content>>(),
        )
        .url("https://katalepsis.net")
        .build()
}