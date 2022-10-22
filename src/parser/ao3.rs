use async_trait::async_trait;
use chrono::{
    naive::NaiveDate,
    offset::{FixedOffset, Local, TimeZone},
};
use regex::Regex;
use select::{
    document::Document,
    predicate::{self, Predicate},
};

use crate::{
    client::get_with_query,
    error::ArchiveError,
    parser::Parser,
    structs::{Author, Chapter, ChapterText, Content, Story, StorySource},
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/chapters/(\d+)", once_cell::sync::OnceCell::new());

pub(crate) struct AO3Parser;

#[async_trait]
impl Parser for AO3Parser {
    async fn get_skeleton(&self, source: StorySource) -> Result<Story, ArchiveError> {
        let main_page = get_with_query(
            &source.to_url(),
            &[("view_adult", "true"), ("view_full_work", "true")],
        )
        .await?
        .text()
        .await?;
        let navigate = get_with_query(
            &format!("{}/navigate", source.to_url()),
            &[("view_adult", "true")],
        )
        .await?
        .text()
        .await?;
        let main_page = Document::from_read(main_page.as_bytes())?;
        let navigate = Document::from_read(navigate.as_bytes())?;

        let name = main_page
            .find(predicate::Class("title").and(predicate::Class("heading")))
            .next()
            .ok_or(ArchiveError::PageError(format!(
                "AO3: Could not find title (.title.heading) for story at {}",
                source.to_url(),
            )))?
            .text();

        let author = main_page
            .find(predicate::Attr("rel", "author").and(predicate::Attr("href", ())))
            .next()
            .ok_or(ArchiveError::PageError(format!(
                "AO3: Could not find author ([rel=\"author\"]) for {} at {}",
                name,
                source.to_url(),
            )))?;
        let author_url = author
            .attr("href")
            .expect("Author link should have href because of find() conditions");
        let author = Author {
            name: author.text(),
            id: format!(
                "ao3{}",
                author_url
                    .replace("/users/", "")
                    .splitn(2, "/pseuds/")
                    .fold(String::new(), |mut acc, s| {
                        acc.push(':');
                        acc.push_str(s);
                        acc
                    }),
            ),
        };

        let description = main_page
            .find(predicate::Class("summary").child(predicate::Class("userstuff")))
            .next()
            .map(|n| n.children().map(|elem| elem.inner_html()).collect());
        let url = source.to_url();
        let tags = get_tags(&main_page);

        let chapters = main_page
            .find(predicate::Attr("id", "chapters").child(predicate::Class("chapter")))
            .map(|chapter| {
                let title_h3 = chapter
                    .descendants()
                    .find(|n| n.is(predicate::Class("title")))
                    .expect("Chapter should have title.");
                let href = title_h3
                    .children()
                    .find_map(|n| n.attr("href"))
                    .expect("Chapter should have link.");
                let name = title_h3.text();
                let mut name_pieces = name.splitn(2, ":");
                let (chapter_num, chapter_name) = (name_pieces.next(), name_pieces.next());
                let name = chapter_name
                    .or(chapter_num)
                    .expect("Chapter should have a name or number")
                    .trim()
                    .to_owned();
                let chap_id = CHAPTER_REGEX
                    .1
                    .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
                    .captures(href)
                    .unwrap()
                    .get(1)
                    .expect("Chapter url must contain id")
                    .as_str();

                let posted_on = navigate
                    .find(predicate::Attr("href", href))
                    .next()
                    .expect("Navigation page should have a link with this chapter's URL")
                    .parent()
                    .unwrap()
                    .children()
                    .find_map(|c| {
                        if c.is(predicate::Class("datetime")) {
                            Some(c.text())
                        } else {
                            None
                        }
                    })
                    .expect("Navigation page should have a datetime span for this chapter");
                let posted_on = posted_on.trim();
                let timezone = FixedOffset::west(Local::now().offset().utc_minus_local());
                let date_posted = timezone
                    .from_local_datetime(
                        &NaiveDate::parse_from_str(&posted_on[1..posted_on.len() - 1], "%F")
                            .expect("Could not parse datestring to date")
                            .and_hms(3, 0, 0),
                    )
                    .earliest()
                    .expect("Could not turn naive to full date");

                let top_notes = chapter
                    .children()
                    .find(|c| c.is(predicate::Attr("id", "notes")));
                let bottom_notes = chapter
                    .children()
                    .find(|c| c.is(predicate::Class("end").and(predicate::Class("notes"))));
                let chapter_text = chapter.children().find(|c| {
                    c.is(predicate::Class("userstuff").and(predicate::Attr("role", "article")))
                });

                let chapter_text = format!(
                    "{}{}{}",
                    top_notes.map(|n| n.inner_html()).unwrap_or_default(),
                    chapter_text
                        .expect("Chapter has no text area")
                        .children()
                        .filter(|node| !node.is(predicate::Attr("id", "work")))
                        .map(|node| node.html())
                        .collect::<String>(),
                    bottom_notes.map(|n| n.inner_html()).unwrap_or_default()
                );

                Content::Chapter(Chapter {
                    id: format!("{}:{}", source.to_id(), chap_id),
                    name,
                    description: None,
                    text: ChapterText::Hydrated(chapter_text),
                    url: format!("https://archiveofourown.org{}", href),
                    date_posted,
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

    async fn fill_skeleton(&self, skeleton: Story) -> Result<Story, ArchiveError> {
        Ok(skeleton)
    }

    async fn get_story(&self, source: StorySource) -> Result<Story, ArchiveError> {
        self.get_skeleton(source).await
    }
}

/// TODO Support series listings and collections at some point?
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
