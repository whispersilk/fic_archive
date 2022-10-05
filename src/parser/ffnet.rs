use html2md::parse_html;
use pandoc::{InputFormat, InputKind, OutputFormat, OutputKind, PandocOutput};
use select::node::Node;
use std::collections::BTreeMap;
use std::iter;

use chrono::DateTime;
use futures::future::join_all;
use rayon::prelude::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};
use reqwest::{Client, Response};
use select::{
    document::Document,
    predicate::{self, Predicate},
};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    structs::{Author, ChapterLink, Content, Story, StoryBase, StorySource, TextFormat},
};

pub fn get_chapter_list(document: &Document, id: &str) -> Result<StoryBase, ArchiveError> {
    let title = document
        .find(predicate::Attr("id", "profile_top").child(predicate::Name("b")))
        .next()
        .expect("Could not find story title")
        .text()
        .trim()
        .to_owned();
    let author = document
        .find(predicate::Attr("id", "profile_top").child(predicate::Attr("href", ())))
        .filter(|node| {
            node.attr("href")
                .expect("Author should have a profile link")
                .starts_with("/u/")
        })
        .next()
        .expect("Could not find author element");
    let author = Author {
        name: author.text().trim().to_owned(),
        id: format!("ffnet:{}", {
            let profile_url = author
                .attr("href")
                .expect("Author should have a profile link");
            &profile_url[3..profile_url.rfind("/").unwrap()]
        }),
    };
    let mut pages: Vec<ChapterLink> = document
        .find(predicate::Attr("id", "chap_select").descendant(predicate::Name("option")))
        .map(|option| ChapterLink {
            url: format!(
                "https://www.fanfiction.net/s/{}/{}",
                id,
                option
                    .attr("value")
                    .expect("A link in the ToC had no value")
                    .to_owned()
            ),
            title: option.text().trim().to_owned(),
        })
        .collect();
    if pages.is_empty() {
        pages.push(ChapterLink {
            url: format!("https://www.fanfiction.net/s/{}/1", id),
            title,
        });
    }

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

    let main_page = Document::from_read(
        runtime
            .block_on(async {
                let intermediate = client.get(&source.to_url()).send().await?;
                intermediate.text().await
            })?
            .as_bytes(),
    )?;
    let chapter_listing = get_chapter_list(&main_page, &source.to_id())?;

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
    let mut chapters: Vec<Content> = runtime
        .block_on(async { join_all(chapter_pages.into_iter().map(|page| page.text())).await })
        .into_par_iter()
        .zip(chapter_urls) // Pair of (text, url)
        .map(|page_plus_url| {
            let document = Document::from_read(page_plus_url.0?.as_bytes())?;
            let title = match chapter_listing.chapter_links.len() {
                1 => chapter_listing.title.to_owned(),
                _ => document.find(predicate::Attr("id", "chap_select").descendant(predicate::Name("option").and(predicate::Attr("selected", ())))).next().expect("Could not find selected chapter").text().trim().to_owned(),
            };
            let body_text: String = document
                .find(predicate::Attr("id", "storytext").child(predicate::Name("p")))
                .map(|elem| elem.html())
                .map(|html| convert_to_format(html, &format))
                .collect();
            let date_posted = document
                .find(predicate::Attr("data-xutime", ()))
                .next()
                .expect("Could not find chapter posted-on date")
                .attr("data-xutime")
                .unwrap();
            let date_posted = TimeZone<FixedOffset>::datetime_from_str(date_posted, "%s").expect(format!("Could not convert timestamp {} to date.", date_posted));
        })
        .map(|c: Result<_, ArchiveError>| c.unwrap())
        .collect();
    chapters.par_sort_unstable_by(|a, b| match (a, b) {
        (
            Content::Chapter {
                date_posted: a_date,
                ..
            },
            Content::Chapter {
                date_posted: b_date,
                ..
            },
        ) => a_date.cmp(b_date),
        _ => unreachable!(),
    });

    let description = main_page
        .find(
            predicate::Attr("id", "profile_top")
            .child(predicate::Name("div").and(predicate::Class("xcontrast_txt")))
        )
        .map(|elem| elem.inner_html())
        .map(|html| convert_to_format(html, &format))
        .collect();
    let tags = main_page
        .find(predicate::Class("tags").child(predicate::Name("a")))
        .map(|elem| elem.text())
        .collect();
    Ok(Story {
        name: chapter_listing.title,
        author: chapter_listing.author,
        description: Some(description),
        url: source.to_url(),
        tags,
        chapters,
        source,
    })
}

fn convert_to_format(html: String, format: &TextFormat) -> String {
    match format {
        TextFormat::Html => html,
        TextFormat::Markdown => {
            let mut pandoc = pandoc::new();
            pandoc
                .set_input_format(InputFormat::Html, Vec::new())
                .set_output_format(OutputFormat::MarkdownStrict, Vec::new())
                .set_input(InputKind::Pipe(html.clone()))
                .set_output(OutputKind::Pipe);
            match pandoc.execute() {
                Ok(PandocOutput::ToBuffer(text)) => text,
                _ => parse_html(html.as_str()),
            }
        }
    }
}
