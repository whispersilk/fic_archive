use chrono::{
    naive::NaiveDate,
    offset::{FixedOffset, Local, TimeZone},
};
use futures::future::join;
use futures::future::join_all;
use regex::Regex;
use reqwest::Client;
use select::{
    document::Document,
    predicate::{self, Predicate},
};
use tokio::runtime::Runtime;

use crate::{
    error::ArchiveError,
    parser::{convert_to_format, Parser},
    structs::{Author, Chapter, ChapterText, Content, Story, StorySource, TextFormat},
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/chapters/(\d+)", once_cell::sync::OnceCell::new());

pub(crate) struct AO3Parser;

impl Parser for AO3Parser {
    fn get_skeleton(
        &self,
        runtime: &Runtime,
        client: &Client,
        format: &TextFormat,
        source: StorySource,
    ) -> Result<Story, ArchiveError> {
        let main_page = async {
            Ok(client
                .get(source.to_url())
                .query(&[("view_adult", "true")])
                .send()
                .await?
                .text()
                .await?)
        };
        let navigate = async {
            Ok(client
                .get(format!("{}/navigate", source.to_url()))
                .query(&[("view_adult", "true")])
                .send()
                .await?
                .text()
                .await?)
        };
        let (main_result, nav_result) = runtime.block_on(async { join(main_page, navigate).await });
        if let Err(e) = main_result {
            return Err(e);
        } else if let Err(e) = nav_result {
            return Err(e);
        }
        let main_page = Document::from_read(main_result.unwrap().as_bytes())?;
        let navigate = Document::from_read(nav_result.unwrap().as_bytes())?;

        let name = main_page
            .find(predicate::Class("title").and(predicate::Class("heading")))
            .next()
            .expect("Story did not have a title")
            .text();
        let author = main_page
            .find(predicate::Attr("rel", "author"))
            .next()
            .expect("Story did not have author");
        let author_url = author
            .attr("href")
            .expect("Author did not have link")
            .replace("/users/", "");
        let mut author_url_split = author_url.splitn(2, "/pseuds/");
        let (base_author, pseud) = (author_url_split.next(), author_url_split.next());
        let author = Author {
            name: author.text(),
            id: format!(
                "ao3:{}:{}",
                base_author.expect("Could not find author"),
                pseud.unwrap_or("")
            ),
        };
        let description = main_page
            .find(predicate::Class("summary").child(predicate::Class("userstuff")))
            .next()
            .map(|n| {
                n.children()
                    .map(|elem| convert_to_format(elem.inner_html(), format))
                    .collect()
            });
        let url = source.to_url();
        let tags = get_tags(&main_page);
        let chapters = navigate
            .find(predicate::Class("chapter").and(predicate::Class("index")))
            .next()
            .expect("Navigation page must have chapter index")
            .children()
            .filter(|node| node.is(predicate::Name("li")))
            .map(|li| {
                let chap = li
                    .children()
                    .find(|c| c.is(predicate::Name("a")))
                    .expect("Chapter should have <a>");
                let href = chap.attr("href").expect("Chapterlink should have link");
                let chap_id = CHAPTER_REGEX
                    .1
                    .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
                    .captures(href)
                    .unwrap()
                    .get(1)
                    .expect("Chapter url must contain id")
                    .as_str();

                let posted_on = li
                    .children()
                    .find(|c| c.is(predicate::Name("span")))
                    .expect("Chapter should have date posted")
                    .text();
                let posted_on = posted_on.trim();
                let timezone = FixedOffset::west(Local::now().offset().utc_minus_local());
                Content::Chapter(Chapter {
                    id: format!("{}:{}", source.to_id(), chap_id),
                    name: chap.text(),
                    description: None,
                    text: ChapterText::Dehydrated,
                    url: format!("https://archiveofourown.org{}", href),
                    date_posted: timezone
                        .from_local_datetime(
                            &NaiveDate::parse_from_str(&posted_on[1..posted_on.len() - 1], "%F")
                                .expect("Could not parse datestring to date")
                                .and_hms(3, 0, 0),
                        )
                        .earliest()
                        .expect("Could not turn naive to full date"),
                })
            })
            .collect();

        Ok(Story {
            name: name.trim().to_owned(),
            author,
            description: description.map(|d: String| d.trim().to_owned()),
            url,
            tags,
            chapters,
            source,
        })
    }

    fn fill_skeleton(
        &self,
        runtime: &Runtime,
        client: &Client,
        format: &TextFormat,
        mut skeleton: Story,
    ) -> Result<Story, ArchiveError> {
        let hydrate = skeleton
            .chapters
            .iter_mut()
            .filter_map(|con| match con {
                Content::Section(_) => None,
                Content::Chapter(c) => Some(c),
            })
            .map(|chapter| async {
                let page = client.get(&chapter.url).send().await?.text().await?;
                Ok((chapter, page))
            });

        let results = runtime.block_on(async { join_all(hydrate).await });
        if results
            .iter()
            .any(|res: &Result<(_, _), ArchiveError>| res.is_err())
        {
            return Err(ArchiveError::Internal("Oopsie!".to_owned()));
        }

        let mut results: Vec<(&mut Chapter, String)> =
            results.into_iter().map(|r| r.unwrap()).collect();
        rayon::scope(|s| {
            for (chapter, page) in results.iter_mut() {
                s.spawn(|_| {
                    let document = Document::from_read(page.as_bytes())
                        .expect("Couldn't read page to a document");
                    let top_notes = document.find(predicate::Attr("id", "notes")).next();
                    let bottom_notes = document
                        .find(predicate::Class("end").and(predicate::Class("notes")))
                        .next();
                    let chapter_text = document
                        .find(predicate::Class("userstuff").and(predicate::Attr("role", "article")))
                        .next();

                    let chapter_text = convert_to_format(
                        format!(
                            "{}{}{}",
                            top_notes.map(|n| n.inner_html()).unwrap_or_default(),
                            chapter_text
                                .expect("Chapter has no text area")
                                .children()
                                .filter(|node| !node.is(predicate::Attr("id", "work")))
                                .map(|node| node.html())
                                .collect::<String>(),
                            bottom_notes.map(|n| n.inner_html()).unwrap_or_default()
                        ),
                        format,
                    );

                    chapter.text = ChapterText::Hydrated(chapter_text);
                });
            }
        });
        Ok(skeleton)
    }

    fn get_story(
        &self,
        runtime: &Runtime,
        format: &TextFormat,
        source: StorySource,
    ) -> Result<Story, ArchiveError> {
        let client = Client::builder().cookie_store(true).build().unwrap();
        let story = self.get_skeleton(runtime, &client, format, source)?;
        self.fill_skeleton(runtime, &client, format, story)
    }
}

fn get_tags(document: &Document) -> Vec<String> {
    document
        .find(
            predicate::Class("work")
                .and(predicate::Class("meta"))
                .and(predicate::Class("group")),
        )
        .next()
        .expect("Story does not have tag box")
        .children()
        .filter(|node| node.is(predicate::Name("dd")))
        .flat_map(|dd| {
            let name = dd
                .attr("class")
                .expect("All dd should have a class attr")
                .replace("tags", "");
            let name = name.trim();
            match name {
                "language" => vec![format!("lang:{}", dd.text().trim())],
                "stats" => Vec::new(),
                _ => dd
                    .descendants()
                    .filter(|node| node.is(predicate::Name("a")))
                    .map(|node| match name {
                        "rating" => format!("rating:{}", node.text().trim().to_lowercase()),
                        "warning" => format!("warning:{}", node.text().trim()),
                        "category" => format!("category:{}", node.text().trim()),
                        "fandom" => format!("fandom:{}", node.text().trim()),
                        "relationship" => format!("relationship:{}", node.text().trim()),
                        "character" => format!("character:{}", node.text().trim()),
                        _ => node.text().trim().to_owned(),
                    })
                    .collect(),
            }
        })
        .collect()
}
