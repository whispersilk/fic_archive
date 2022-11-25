use async_trait::async_trait;
use chrono::DateTime;
use futures::future::join_all;
use regex::Regex;
use select::{document::Document, predicate, predicate::Predicate};

use crate::{
    client::get,
    error::ArchiveError,
    parser::Parser,
    structs::{Author, AuthorList, Chapter, ChapterText, Completed, Content, Story, StorySource},
    Result,
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/chapter/(\d+)", once_cell::sync::OnceCell::new());

pub(crate) struct RoyalRoadParser;

#[async_trait]
impl Parser for RoyalRoadParser {
    async fn get_skeleton(&self, source: StorySource) -> Result<Story> {
        let main_page = get(&source.to_url()).await?.text().await?;
        let main_page = Document::from_read(main_page.as_bytes())?;
        let chapters = main_page
            .find(
                predicate::Attr("id", "chapters")
                    .child(predicate::Name("tbody"))
                    .child(predicate::Name("tr")),
            )
            .map(|row| {
                let content_a = row
                    .children()
                    .filter(|c| c.is(predicate::Name("td")))
                    .filter(|c| c.attr("data-content").is_some())
                    .map(|tr| {
                        tr.children()
                            .find(|c| c.is(predicate::Name("a")))
                            .expect("Should have a node with chapter post time")
                    })
                    .next()
                    .expect("Should have a td with data-content");

                let name = row
                    .children()
                    .filter(|c| c.is(predicate::Name("td")))
                    .filter(|c| c.attr("data-content").is_none())
                    .map(|tr| {
                        tr.children()
                            .find(|c| c.is(predicate::Name("a")))
                            .expect("Should have a node with chapter link")
                            .text()
                            .trim()
                            .to_owned()
                    })
                    .next()
                    .expect("Should have a td without data-content");
                let url = format!(
                    "https://www.royalroad.com{}",
                    content_a
                        .attr("href")
                        .expect("A link in the ToC had no href")
                        .to_owned()
                );
                let time_string = content_a
                    .children()
                    .filter(|c| c.is(predicate::Name("time")))
                    .map(|c| {
                        c.attr("datetime")
                            .expect("time tag should have datetime attr")
                    })
                    .next()
                    .expect("Chapter content tag should have <time> child");
                let date_posted = DateTime::parse_from_rfc3339(time_string).unwrap_or_else(|_| {
                    panic!(
                        "Chapter posted-on date ({}) did not conform to rfc3339",
                        time_string
                    )
                });

                Content::Chapter(Chapter {
                    id: format!(
                        "{}:{}",
                        source.to_id(),
                        CHAPTER_REGEX
                            .1
                            .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
                            .captures(&url)
                            .unwrap()
                            .get(1)
                            .expect("Chapter url must contain id")
                            .as_str()
                    ),
                    name,
                    description: None,
                    text: ChapterText::Dehydrated,
                    url,
                    date_posted,
                    author: None,
                })
            })
            .collect();
        let title = main_page
            .find(predicate::Class("fic-title").descendant(predicate::Name("h1")))
            .next()
            .expect("Could not find story title")
            .text();
        let author =
            main_page
                .find(predicate::Class("fic-title").descendant(
                    predicate::Attr("property", "author").descendant(predicate::Name("a")),
                ))
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
        let description = main_page
            .find(
                predicate::Class("hidden-content").and(predicate::Attr("property", "description")),
            )
            .map(|elem| elem.inner_html())
            .collect();
        let tags = main_page
            .find(predicate::Class("tags").child(predicate::Name("a")))
            .map(|elem| elem.text())
            .collect();
        let completed = get_completed(&main_page, &source)?;

        Ok(Story {
            name: title,
            authors: AuthorList::new(author),
            description: Some(description),
            url: source.to_url(),
            tags,
            chapters,
            source,
            completed,
        })
    }

    async fn fill_skeleton(&self, mut skeleton: Story) -> Result<Story> {
        let hydrate = skeleton
            .chapters
            .iter_mut()
            .filter_map(|con| match con {
                Content::Section(_) => None,
                Content::Chapter(c) => Some(c),
            })
            .map(|chapter| async {
                let page = get(&chapter.url).await?.text().await?;
                Ok((chapter, page))
            });

        let results = join_all(hydrate).await;
        if results.iter().any(|res: &Result<(_, _)>| res.is_err()) {
            return Err(ArchiveError::Internal("Oopsie!".to_owned()));
        }

        let mut results: Vec<(&mut Chapter, String)> =
            results.into_iter().map(|r| r.unwrap()).collect();
        rayon::scope(|s| {
            for (chapter, page) in results.iter_mut() {
                s.spawn(|_| {
                    let document = Document::from_read(page.as_bytes())
                        .expect("Couldn't read page to a document");
                    let body_text = document
                        .find(predicate::Class("chapter-content").child(predicate::Name("p")))
                        .map(|elem| elem.html())
                        .collect();
                    chapter.text = ChapterText::Hydrated(body_text);
                });
            }
        });
        Ok(skeleton)
    }

    async fn get_story(&self, source: StorySource) -> Result<Story> {
        let story = self.get_skeleton(source).await?;
        self.fill_skeleton(story).await
    }
}

fn get_completed(document: &Document, source: &StorySource) -> Result<Completed> {
    Ok(document
        .find(predicate::Class("fiction-info"))
        .next()
        .ok_or(ArchiveError::PageError(format!(
            "Royalroad: couldn't find story info panel (.fiction-info) for story at {}",
            source.to_url()
        )))?
        .descendants()
        .find(|d| {
            d.is(predicate::Class("label")) && {
                d.text().trim() == "ONGOING" || d.text().trim() == "COMPLETED"
            }
        })
        .map(|d| match d.text().trim() {
            "COMPLETED" => Completed::Complete,
            "ONGOING" => Completed::Incomplete,
            _ => unreachable!(),
        })
        .unwrap_or(Completed::Unknown))
}
