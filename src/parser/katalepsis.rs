use html2md::parse_html;
use pandoc::{InputFormat, InputKind, OutputFormat, OutputKind, PandocOutput};
use std::collections::BTreeMap;

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
    structs::{Author, ChapterLink, Content, Story, StorySource, TextFormat},
};

pub fn get_chapter_list(
    runtime: &Runtime,
    client: &Client,
) -> Result<Vec<ChapterLink>, ArchiveError> {
    let main_page = runtime.block_on(async {
        let intermediate = client
            .get("https://katalepsis.net/table-of-contents/")
            .send()
            .await?;
        intermediate.text().await
    })?;

    let document = Document::from_read(main_page.as_bytes())?;
    let pages = document
        .find(predicate::Name("div"))
        .into_selection()
        .find(predicate::Class("entry-content"))
        .find(predicate::Name("a"))
        .into_iter()
        .map(|link| ChapterLink {
            url: link
                .attr("href")
                .expect("A link in the ToC had no href")
                .to_owned(),
            title: link.text().trim().to_owned(),
        })
        .collect();
    Ok(pages)
}

pub fn get_story(runtime: &Runtime, format: TextFormat) -> Result<Story, ArchiveError> {
    let client = Client::new();

    let chapter_listing = get_chapter_list(runtime, &client)?;
    let pages = chapter_listing
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
    let sections: Vec<Content> = runtime
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
                .find(predicate::Class("entry-content").child(predicate::Class("western")).child(predicate::Name("span")))
                .map(|elem| match format {
                    TextFormat::Html => elem.inner_html(),
                    TextFormat::Markdown => {
                        let mut pandoc = pandoc::new();
                        pandoc
                            .set_input_format(InputFormat::Html, Vec::new())
                            .set_output_format(OutputFormat::MarkdownStrict, Vec::new())
                            .set_input(InputKind::Pipe(elem.inner_html()))
                            .set_output(OutputKind::Pipe);
                        match pandoc.execute() {
                            Ok(PandocOutput::ToBuffer(text)) => text,
                            _ => parse_html(elem.html().as_str()),
                        }
                    }
                })
                .collect();
            let date_posted = document
                .find(predicate::Class("entry-date"))
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
        .fold(
            BTreeMap::new(),
            // Use (u16, String) tuple as key so order is maintained according to u16 value.
            |mut acc: BTreeMap<(u16, String), Vec<Content>>, chapter| {
                let arc_info = match chapter {
                    Content::Chapter {
                        ref name,
                        description: _,
                        text: _,
                        url: _,
                        date_posted: _,
                    } => {
                        let idx = name
                            .find(" –")
                            .unwrap_or_else(|| panic!("Did not find pattern in {}", name));
                        let pieces = name.split_at(idx);
                        let chapter_name = pieces.0.to_owned();
                        let number = pieces
                            .1
                            .find(|c: char| c.is_ascii_digit())
                            .unwrap_or_else(|| panic!("Did not find digit in {}", pieces.1));
                        let number = pieces.1.split_at(number).1;
                        let number = number.split_at(number.find('.').unwrap()).0;
                        let number = number
                            .parse::<u16>()
                            .unwrap_or_else(|_| panic!("{} should be an int", number));
                        (number, format!("Arc {}: {}", number, chapter_name))
                    }
                    _ => unreachable!("All Content at this point are Chapters"),
                };
                if acc.get(&arc_info).is_none() {
                    let new_vec = Vec::new();
                    acc.insert((arc_info.0, arc_info.1.clone()), new_vec);
                }
                acc.get_mut(&arc_info).unwrap().push(chapter);
                acc
            },
        )
        .into_iter()
        .map(|((_arc_num, arc_name), chapters)| Content::Section {
            name: arc_name,
            description: None,
            chapters,
            url: None,
        })
        .collect();

    Ok(Story {
        name: "Katalepsis".to_owned(),
        author: Author {
            name: "HY".to_owned(),
            id: "katalepsis:".to_owned(),
        },
        description: None,
        url: "https://katalepsis.net".to_owned(),
        tags: Vec::new(),
        chapters: sections,
        source: StorySource::Katalepsis,
    })
}
