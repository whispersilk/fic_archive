use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, TimeZone};
use futures::future::join_all;
use select::{
    document::Document,
    node::Data::Text,
    node::Node,
    predicate::{self, Predicate},
};

use std::iter;

use crate::{
    client::get,
    parser::Parser,
    structs::{
        Author, AuthorList, Chapter, ChapterText, Completed, Content, Section, Story, StorySource,
    },
    Result,
};

pub(crate) struct KatalepsisParser;

#[async_trait]
impl Parser for KatalepsisParser {
    async fn get_skeleton(&self, source: StorySource) -> Result<Story> {
        let main_page = get(&source.to_url()).await?.text().await?;
        let main_page = Document::from_read(main_page.as_bytes())?;

        let name = "Katalepsis".to_owned();
        let author = Author::new("HY", "katalepsis:");
        let description: Option<String> = Some(
            main_page
                .find(predicate::Class("entry-content").child(predicate::Name("p")))
                .take(3)
                .map(|elem| elem.inner_html())
                .collect(),
        );
        let url = source.to_url();
        let tags: Vec<String> = Vec::new();
        let chapters: Vec<Content> = main_page
            .find(predicate::Attr("id", "secondary").child(predicate::Name("aside")))
            .find(|node| match node.first_child() {
                None => false,
                Some(child) => {
                    child.is(predicate::Name("h3")) && child.text().as_str() == "Archive"
                }
            })
            .expect("Could not find post archive in right-hand panel")
            .children()
            .find(|node| matches!(node.attr("class"), Some("textwidget")))
            .expect("Post archive is empty")
            .children()
            .filter(|child| child.is(predicate::Name("ul")))
            .flat_map(|arc_ul| arc_ul.children())
            .filter(|arc_li| arc_li.name().is_some())
            .map(|arc_li| {
                let arc_name = arc_li
                    .children()
                    .find(|child| matches!(child.data(), Text(_)));
                if arc_name.is_none() {
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
                    .find(|child| child.is(predicate::Name("ul")))
                    .expect("<li> for arc should have a <ul> for chapters")
                    .children()
                    .filter(|chapter_li| match chapter_li.first_child() {
                        Some(child) => child.is(predicate::Name("a")),
                        None => false,
                    })
                    .map(|chapter_li| chapter_li.first_child().unwrap())
                    .map(|a_tag| {
                        let chap_num_owner = a_tag.text();
                        let chap_num = chap_num_owner.split('.').nth(1).unwrap_or_else(|| {
                            panic!(
                                "Chapter number should be of the format X.Y but is {}",
                                a_tag.text()
                            )
                        });
                        Content::Chapter(Chapter {
                            id: format!("katalepsis:{}:{}", arc_num, chap_num),
                            name: format!("{} - {}", arc_name, a_tag.text()),
                            description: None,
                            text: ChapterText::Dehydrated,
                            url: a_tag
                                .attr("href")
                                .expect("Chapter tag should have an href")
                                .to_owned(),
                            date_posted: FixedOffset::east(0).datetime_from_str("0", "%s").unwrap(),
                            author: None,
                        })
                    })
                    .collect();
                Content::Section(Section {
                    id: format!("katalepsis:{}", arc_num),
                    name: arc_name,
                    description: None,
                    chapters,
                    url: None,
                    author: None,
                })
            })
            .collect();

        Ok(Story {
            name,
            authors: AuthorList::new(author),
            description,
            url,
            tags,
            chapters,
            source,
            completed: Completed::Incomplete,
        })
    }

    async fn fill_skeleton(&self, mut skeleton: Story) -> Result<Story> {
        let mut chapters: Vec<&mut Chapter> = Vec::with_capacity(skeleton.num_chapters());
        for content in skeleton.chapters.iter_mut() {
            match content {
                Content::Section(ref mut sec) => chapters_from_section(sec, &mut chapters),
                Content::Chapter(ref mut chap) => chapters.push(chap),
            }
        }

        let hydrate = chapters.into_iter().map(|chap| async {
            let page = get(&chap.url).await?.text().await?;
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
                .map(|chap| {
                    chap.inner_html()
                        .replace(">* * *<", " align=\"center\">* * *<")
                        .replace("==", "<span align=\"center\">* * *</span>")
                });
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

            let body_text = ChapterText::Hydrated(
                content_warnings
                    .chain(chapter_paragraphs)
                    .chain(a_n_paragraphs)
                    .filter(|html| {
                        !html.contains(">Previous Chapter<") && !html.contains(">Next Chapter<")
                    })
                    .collect(),
            );
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

        let results = join_all(hydrate).await;
        match results.into_iter().find(|res| res.is_err()) {
            Some(err) => Err(err.unwrap_err()),
            None => Ok(skeleton),
        }
    }

    async fn get_story(&self, source: StorySource) -> Result<Story> {
        let story = self.get_skeleton(source).await?;
        self.fill_skeleton(story).await
    }
}

fn chapters_from_section<'a>(section: &'a mut Section, vec: &mut Vec<&'a mut Chapter>) {
    for content in section.chapters.iter_mut() {
        match content {
            Content::Section(ref mut sec) => chapters_from_section(sec, vec),
            Content::Chapter(ref mut chap) => vec.push(chap),
        }
    }
}
