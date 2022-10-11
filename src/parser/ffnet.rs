use chrono::{offset::FixedOffset, TimeZone};
use futures::future::join_all;
use rayon::prelude::{
    IndexedParallelIterator, IntoParallelIterator, ParallelIterator, ParallelSliceMut,
};
use regex::Regex;
use reqwest::{Client, Response};
use select::{
    document::Document,
    predicate::{self, Predicate},
};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    parser::convert_to_format,
    structs::{Author, Chapter, ChapterLink, Content, Story, StoryBase, StorySource, TextFormat},
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/s/\d+/(\d+)", once_cell::sync::OnceCell::new());

// Need to get the challenge URL and send a post to that to get a cookie?

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
            title: title.clone(),
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

    let main_page = Document::from_read({
        let page_bytes = runtime.block_on(async {
            let intermediate = client.get(&source.to_url()).send().await?;
            intermediate.text().await
        })?;
        println!("{:?}", page_bytes);
        page_bytes.clone().as_bytes()
    })?;
    let chapter_listing = get_chapter_list(&main_page, &source.to_id())?;

    let num_chapters = chapter_listing.chapter_links.len();
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
    let mut chapters: Vec<Chapter> = runtime
        .block_on(async { join_all(chapter_pages.into_iter().map(|page| page.text())).await })
        .into_par_iter()
        .zip(chapter_urls) // Pair of (text, url)
        .map(|page_plus_url| {
            let document = Document::from_read(page_plus_url.0?.as_bytes())?;
            let title =
                match num_chapters {
                    1 => chapter_listing.title.to_owned(),
                    _ => document
                        .find(predicate::Attr("id", "chap_select").descendant(
                            predicate::Name("option").and(predicate::Attr("selected", ())),
                        ))
                        .next()
                        .expect("Could not find selected chapter")
                        .text()
                        .trim()
                        .to_owned(),
                };
            let body_text: String = document
                .find(predicate::Attr("id", "storytext").child(predicate::Name("p")))
                .map(|elem| elem.html())
                .map(|html| convert_to_format(html, &format))
                .collect();
            let date_posted = document
                .find(predicate::Attr("data-xutime", ()))
                .next() // Gets story "updated on" date if exists, else "published on"
                .expect("Could not find chapter posted-on date")
                .attr("data-xutime")
                .unwrap();
            let date_posted = FixedOffset::east(0)
                .datetime_from_str(date_posted, "%s")
                .expect(&format!(
                    "Could not convert timestamp {} to date.",
                    date_posted
                ));
            Ok(Chapter {
                id: format!(
                    "ffnet:{}:{}",
                    if let StorySource::FFNet(ref id) = &source {
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
                date_posted,
            })
        })
        .map(|c: Result<_, ArchiveError>| c.unwrap())
        .collect();
    chapters.par_sort_unstable_by(|a, b| a.date_posted.cmp(&b.date_posted));

    let description = main_page
        .find(
            predicate::Attr("id", "profile_top")
                .child(predicate::Name("div").and(predicate::Class("xcontrast_txt"))),
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
        chapters: chapters.into_iter().map(Content::Chapter).collect(),
        source,
    })
}
