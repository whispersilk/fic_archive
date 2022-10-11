use tokio::io::AsyncWriteExt;
use chrono::{DateTime, FixedOffset, TimeZone};
use futures::future::join_all;
use reqwest::Client;
use select::{
    document::Document,
    node::Data::Text,
    node::Node,
    predicate::{self, Predicate},
};
use tokio::runtime::Runtime;

use std::iter;

use crate::{
    error::ArchiveError,
    parser::{custom_convert_to_format, Parser},
    structs::{Author, Chapter, Content, Section, Story, StorySource, TextFormat},
};

pub(crate) struct KatalepsisParser;

fn chapters_from_section<'a>(section: &'a mut Section, vec: &mut Vec<&'a mut Chapter>) -> () {
    for content in section.chapters.iter_mut() {
        match content {
            Content::Section(sec) => chapters_from_section(sec, vec),
            Content::Chapter(chap) => vec.push(chap),
        }
    }
}

impl Parser for KatalepsisParser {
    fn get_skeleton(
        &self,
        runtime: &Runtime,
        client: &Client,
        format: &TextFormat,
        source: StorySource,
    ) -> Result<Story, ArchiveError> {
        let main_page = runtime.block_on(async {
            client
                .get("https://katalepsis.net/")
                .send()
                .await?
                .text()
                .await
        })?;
        let main_page = Document::from_read(main_page.as_bytes())?;

        let name = "Katalepsis".to_owned();
        let author = Author::new("HY", "katalepsis:");
        let description: Option<String> = Some(
            main_page
                .find(predicate::Class("entry-content").child(predicate::Name("p")))
                .take(3)
                .map(|elem| elem.inner_html())
                .map(|html| custom_convert_to_format(html, &format, Some(Box::new(custom_convert))))
                .collect(),
        );
        let url = "https://katalepsis.net".to_owned();
        let tags: Vec<String>= Vec::new();
        let chapters: Vec<Content> = main_page
            .find(predicate::Attr("id", "secondary").child(predicate::Name("aside")))
            .filter(|node| match node.first_child() {
                None => false,
                Some(child) => match (child.name(), child.text().as_str()) {
                    (Some("h3"), "Archive") => true,
                    _ => false,
                },
            })
            .next()
            .expect("Could not find post archive in right-hand panel")
            .children()
            .filter(|node| if let Some("textwidget") = node.attr("class") { true } else { false })
            .next()
            .expect("Post archive is empty")
            .children()
            .filter(|child| {
                if let Some("ul") = child.name() {
                    true
                } else {
                    false
                }
            })
            .flat_map(|arc_ul| arc_ul.children())
            .filter(|arc_li| arc_li.name().is_some())
            .map(|arc_li| {
                let arc_name = arc_li
                    .children()
                    .filter(|child| {
                        if let Text(_) = child.data() {
                            true
                        } else {
                            false
                        }
                    })
                    .next();
                if let None = arc_name {
                    println!("Arc name was none for:\n{:?}", arc_li);
                }
                let arc_name = arc_name
                    .expect("<li> for arc should have a text node with arc name")
                    .text()
                    .replacen('(', "", 1)
                    .replacen(')', ":", 1);
                let arc_num = &arc_name[4..arc_name.find(':').unwrap()];
                let chapters = arc_li
                    .children()
                    .filter(|child| {
                        if let Some("ul") = child.name() {
                            true
                        } else {
                            false
                        }
                    })
                    .next()
                    .expect("<li> for arc should have a <ul> for chapters")
                    .children()
                    .filter(|chapter_li| match chapter_li.first_child() {
                        Some(child) => {
                            if let Some("a") = child.name() {
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    })
                    .map(|chapter_li| chapter_li.first_child().unwrap())
                    .map(|a_tag| {
                        let chap_num_owner = a_tag.text();
                        let chap_num = chap_num_owner.split(".").skip(1).next().expect(&format!(
                            "Chapter number should be of the format X.Y but is {}",
                            a_tag.text()
                        ));
                        Content::Chapter(Chapter {
                            id: format!("katalepsis:{}:{}", arc_num, chap_num),
                            name: format!("{} - {}", arc_name, a_tag.text()),
                            description: None,
                            text: String::new(),
                            url: a_tag
                                .attr("href")
                                .expect("Chapter tag should have an href")
                                .to_owned(),
                            date_posted: FixedOffset::east(0).datetime_from_str("0", "%s").unwrap(),
                        })
                    })
                    .collect();
                Content::Section(Section {
                    id: format!("katalepsis:{}", arc_num),
                    name: arc_name,
                    description: None,
                    chapters,
                    url: None,
                })
            })
            .collect();

        Ok(Story {
            name,
            author,
            description,
            url,
            tags,
            chapters,
            source,
        })
    }

    fn get_story(
        &self,
        runtime: &Runtime,
        format: &TextFormat,
        source: StorySource,
    ) -> Result<Story, ArchiveError> {
        let client = Client::new();
        let mut story = self.get_skeleton(runtime, &client, &format, source).unwrap();

        let mut chapters: Vec<&mut Chapter> = Vec::with_capacity(story.num_chapters());
        for content in story.chapters.iter_mut() {
            match content {
                Content::Section(sec) => chapters_from_section(sec, &mut chapters),
                Content::Chapter(chap) => chapters.push(chap),
            }
        }
        println!("Chapters: {}", chapters.len());
        for chap in chapters.iter() {
            println!("Chapter name is: \"{}\"", chap.name);
        }

        let hydrate = chapters.into_iter().map(|chap| async {
            tokio::io::stdout().write_all(format!("About to fetch chapter {}\n", chap.name).as_bytes()).await.unwrap();
            let page = client.get(&chap.url).send().await?.text().await?;
            tokio::io::stdout().write_all(format!("Done fetching chapter {}\n", chap.name).as_bytes()).await.unwrap();
            let document = Document::from_read(page.as_bytes())?;

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
                .map(|html| custom_convert_to_format(html, &format, Some(Box::new(custom_convert))))
                .collect();
            let date_posted = document
                .find(predicate::Class("entry-date"))
                .next()
                .expect("Could not find chapter posted-on date")
                .attr("datetime")
                .expect("Could not find chapter posted-on date attr");
            let date_posted = DateTime::parse_from_rfc3339(date_posted).unwrap_or_else(|_| {
                panic!(
                    "Chapter posted-on date ({}) did not conform to rfc3339",
                    date_posted
                )
            });
            
            chap.text = body_text;
            chap.date_posted = date_posted;
            Ok(())
        });
        let results = runtime.block_on(async { join_all(hydrate).await });
        match results.into_iter().find(|res| res.is_err()) {
            Some(err) => Err(err.unwrap_err()),
            None => Ok(story),
        }
    }
}

fn custom_convert(formatted_text: String, format: &TextFormat) -> String {
    match format {
        TextFormat::Markdown => match formatted_text.as_ref() {
            "==" => "<span align=\"center\">* * *</span>".to_owned(),
            _ => {
                if formatted_text.contains(">* * *<") {
                    formatted_text.replace(">* * *<", " align=\"center\">* * *<")
                } else {
                    formatted_text
                }
            }
        },
        _ => formatted_text,
    }
}
