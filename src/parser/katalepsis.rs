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
        .find(predicate::Class("entry-content").descendant(predicate::Name("a")))
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
        .into_par_iter()
        .zip(chapter_urls) // Pair of (text, url)
        .map(|page_plus_url| {
            let document = Document::from_read(page_plus_url.0?.as_bytes())?;
            let title = document
                .find(predicate::Class("entry-header").child(predicate::Class("entry-title")))
                .next()
                .expect("Chapter does not have title")
                .text();

            let mut cw_empty_owner;
            let mut cw_some_owner;
            let content_warnings: &mut dyn Iterator<Item = String> = match document
                .find(
                    predicate::Class("entry-content")
                        .child(predicate::Name("details"))
                        .child(predicate::Name("p")),
                )
                .next()
            {
                Some(node) => {
                    if node.text().trim().starts_with("None") {
                        cw_empty_owner = iter::empty::<String>();
                        &mut cw_empty_owner
                    } else {
                        cw_some_owner = iter::once(format!(
                            "<b>Content Warnings:</b><br>{}",
                            node.inner_html().trim()
                        ));
                        &mut cw_some_owner
                    }
                }
                None => {
                    cw_empty_owner = iter::empty::<String>();
                    &mut cw_empty_owner
                }
            };
            let body_elems: Vec<Node> = document
                .find(predicate::Class("entry-content").child(predicate::Name("p")))
                .collect();
            let mut chapter_start_index: Option<usize> = None;
            let mut chapter_end_index: Option<usize> = None;
            for (idx, elem) in body_elems.iter().enumerate() {
                if elem.inner_html().contains(">Previous Chapter<")
                    || elem.inner_html().contains(">Next Chapter<")
                {
                    if chapter_start_index.is_none() {
                        chapter_start_index = Some(idx);
                    } else {
                        chapter_end_index = Some(idx);
                    }
                }
            }
            let chapter_start_index = chapter_start_index.unwrap();
            let chapter_end_index = chapter_end_index.unwrap();
            let chapter_paragraphs = body_elems[chapter_start_index + 1..chapter_end_index]
                .iter()
                .map(|chap| chap.inner_html());
            let mut a_n_empty_owner;
            let mut a_n_some_owner;
            let a_n_paragraphs: &mut dyn Iterator<Item = String> =
                if chapter_end_index == body_elems.len() - 1 {
                    a_n_empty_owner = iter::empty::<String>();
                    &mut a_n_empty_owner
                } else {
                    a_n_some_owner = iter::once("<b>Author's Notes:</b>".to_owned()).chain(
                        body_elems[chapter_end_index + 1..]
                            .iter()
                            .map(|chap| chap.inner_html()),
                    );
                    &mut a_n_some_owner
                };

            let body_text = content_warnings
                .chain(chapter_paragraphs)
                .chain(a_n_paragraphs)
                .filter(|html| {
                    !html.contains(">Previous Chapter<") && !html.contains(">Next Chapter<")
                })
                .map(|html| convert_to_format(html, &format))
                .collect();
            let date_posted = document
                .find(predicate::Class("entry-date"))
                .next()
                .expect("Could not find chapter posted-on date")
                .attr("datetime")
                .expect("Could not find chapter posted-on date attr");
            let id = {
                let idx = title
                    .find(" –")
                    .unwrap_or_else(|| panic!("Did not find pattern in {}", title));
                let pieces = title.split_at(idx);
                let number = pieces
                    .1
                    .find(|c: char| c.is_ascii_digit())
                    .unwrap_or_else(|| panic!("Did not find digit in {}", pieces.1));
                let number = pieces.1.split_at(number).1;
                let num_pieces = number.split_at(number.find('.').unwrap());
                let (arc_num, chap_num) = (num_pieces.0, &num_pieces.1[1..]);
                format!("katalepsis:{}:{}", arc_num, chap_num)
            };
            Ok(Content::Chapter {
                id,
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
        .collect::<Vec<Content>>()
        .into_iter()
        .map(|chapter| {
            let id = if let Content::Chapter { id, .. } = &chapter {
                id
            } else {
                unreachable!()
            };
            let arc_num = &id[id.find(':').unwrap() + 1..id.rfind(':').unwrap()];
            let arc_num = arc_num
                .parse::<u8>()
                .unwrap_or_else(|_| panic!("Arc number '{}' should be an int", arc_num));
            (arc_num, chapter)
        })
        .fold(
            BTreeMap::new(),
            // Use (u16, String) tuple as key so order is maintained according to u16 value.
            |mut acc: BTreeMap<(u8, String), Vec<Content>>, pair| {
                let arc_num = pair.0;
                let chapter = pair.1;
                let arc_info = match chapter {
                    Content::Chapter { ref name, .. } => {
                        let idx = name
                            .find(" –")
                            .unwrap_or_else(|| panic!("Did not find pattern in {}", name));
                        let chapter_name = name.split_at(idx).0.to_owned();
                        (arc_num, format!("Arc {}: {}", arc_num, chapter_name))
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
        .map(|((arc_num, arc_name), mut chapters)| {
            chapters.sort_unstable_by(|chap1, chap2| match (chap1, chap2) {
                (Content::Chapter { id: id1, .. }, Content::Chapter { id: id2, .. }) => {
                    let chap1_id = &id1[id1.rfind(':').unwrap() + 1..];
                    let chap1_id = chap1_id
                        .parse::<u8>()
                        .unwrap_or_else(|_| panic!("Arc number '{}' should be an int", chap1_id));
                    let chap2_id = &id2[id2.rfind(':').unwrap() + 1..];
                    let chap2_id = chap2_id
                        .parse::<u8>()
                        .unwrap_or_else(|_| panic!("Arc number '{}' should be an int", chap2_id));
                    chap1_id.cmp(&chap2_id)
                }
                _ => unreachable!("All Section chapters are of type Chapter"),
            });
            Content::Section {
                id: format!("katalepsis:{}", arc_num),
                name: arc_name,
                description: None,
                chapters,
                url: Some(format!("https://katalepsis.net/category/arc-{}/", arc_num)),
            }
        })
        .collect();

    let home_page = Document::from_read(
        runtime
            .block_on(async {
                let intermediate = client.get("https://katalepsis.net/").send().await?;
                intermediate.text().await
            })?
            .as_bytes(),
    )?;

    let description: String = home_page
        .find(predicate::Class("entry-content").child(predicate::Name("p")))
        .take(3)
        .map(|elem| elem.inner_html())
        .map(|html| convert_to_format(html, &format))
        .collect();

    Ok(Story {
        name: "Katalepsis".to_owned(),
        author: Author {
            name: "HY".to_owned(),
            id: "katalepsis:".to_owned(),
        },
        description: Some(description),
        url: "https://katalepsis.net".to_owned(),
        tags: Vec::new(),
        chapters: sections,
        source: StorySource::Katalepsis,
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
                Ok(PandocOutput::ToBuffer(text)) => match text.as_ref() {
                    "==" => "<span align=\"center\">* * *</span>".to_owned(),
                    _ => {
                        if text.contains(">* * *<") {
                            text.replace(">* * *<", " align=\"center\">* * *<")
                        } else {
                            text
                        }
                    }
                },
                _ => parse_html(html.as_str()),
            }
        }
    }
}
