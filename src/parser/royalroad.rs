use html2md::parse_html;
use pandoc::{InputFormat, InputKind, OutputFormat, OutputKind, PandocOutput};
use select::predicate::Predicate;

use chrono::DateTime;
use futures::future::join_all;
use rayon::prelude::{
    IndexedParallelIterator, IntoParallelIterator, ParallelIterator, ParallelSliceMut,
};
use regex::Regex;
use reqwest::{Client, Response};
use select::{document::Document, predicate};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    structs::{Author, ChapterLink, Content, Story, StoryBase, StorySource, TextFormat},
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/chapter/(\d+)", once_cell::sync::OnceCell::new());

pub fn get_chapter_list(document: &Document) -> Result<StoryBase, ArchiveError> {
    let pages = document
        .find(
            predicate::Attr("id", "chapters")
                .child(predicate::Name("tbody"))
                .descendant(predicate::Name("a")),
        )
        .filter(|node| node.attr("data-content") == None)
        .inspect(|x| println!("node is: {:?}", x))
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
        .find(predicate::Class("fic-title").descendant(predicate::Name("h1")))
        .next()
        .expect("Could not find story title")
        .text();
    let author = document
        .find(
            predicate::Class("fic-title")
                .descendant(predicate::Attr("property", "author").descendant(predicate::Name("a"))),
        )
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

    let main_page = Document::from_read(
        runtime
            .block_on(async {
                let intermediate = client.get(&source.to_url()).send().await?;
                intermediate.text().await
            })?
            .as_bytes(),
    )?;
    let chapter_listing = get_chapter_list(&main_page)?;

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
            let title = document
                .find(predicate::Class("fic-header").descendant(predicate::Name("h1")))
                .next()
                .expect("Chapter does not have title")
                .text();
            let body_text: String = document
                .find(predicate::Class("chapter-content").child(predicate::Name("p")))
                .map(|elem| elem.html())
                .map(|html| convert_to_format(html, &format))
                .collect();
            let date_posted = document
                .find(predicate::Class("fa-calendar"))
                .next()
                .expect("Could not find chapter posted-on date")
                .parent()
                .unwrap()
                .children()
                .find(|node| node.attr("datetime").is_some())
                .expect("Could not find chapter posted-on date")
                .attr("datetime")
                .unwrap();
            Ok(Content::Chapter {
                id: format!(
                    "rr:{}:{}",
                    if let StorySource::RoyalRoad(ref id) = &source {
                        id
                    } else {
                        unreachable!()
                    },
                    CHAPTER_REGEX
                        .1
                        .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
                        .captures(&page_plus_url.1)
                        .unwrap()
                        .get(1)
                        .expect("Chapter url must contain id")
                        .as_str()
                ),
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
            predicate::Class("hidden-content")
                .and(predicate::Attr("property", "description"))
                .child(predicate::Name("p")),
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
