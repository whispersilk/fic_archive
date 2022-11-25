use async_trait::async_trait;
use chrono::{
    naive::NaiveDate,
    offset::{FixedOffset, Local, TimeZone},
    DateTime,
};
use regex::Regex;
use select::{
    document::Document,
    node::Node,
    predicate::{self, Predicate},
};

use crate::{
    client::get_with_query,
    error::ArchiveError,
    parser::Parser,
    structs::{Author, AuthorList, Chapter, ChapterText, Completed, Content, Story, StorySource},
    Result,
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/chapters/(\d+)", once_cell::sync::OnceCell::new());

pub(crate) struct AO3Parser;

#[async_trait]
impl Parser for AO3Parser {
    async fn get_skeleton(&self, source: StorySource) -> Result<Story> {
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
            .text()
            .trim()
            .to_owned();

        let author = main_page
            .find(predicate::Attr("rel", "author").and(predicate::Attr("href", ())))
            .next()
            .ok_or(ArchiveError::PageError(format!(
                "AO3: Could not find author ([rel=\"author\" href]) for story at {}",
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
        let completed = get_completed(&main_page, &source);

        let chapters = main_page
            .find(predicate::Attr("id", "chapters"))
            .next()
            .ok_or(ArchiveError::PageError(format!(
                "AO3: Could not find chapter section ([id=\"chapters\"]) for story at {}",
                source.to_url()
            )))?;
        let mut children = chapters
            .children()
            .filter(|c| c.is(predicate::Class("chapter")))
            .peekable();
        let chapters = if children.peek().is_some() {
            children
                .map(|chapter| {
                    let url = get_chapter_url(&chapter, &source)?;
                    let id = get_chapter_id(&chapter, &source)?;
                    let name = get_chapter_name(&chapter, &source)?;
                    let date_posted = get_chapter_date_posted(&navigate, &url, &source)?;
                    let text = get_chapter_text(&chapter, &url)?;
                    Ok(Content::Chapter(Chapter {
                        id,
                        name,
                        description: None,
                        text: ChapterText::Hydrated(text),
                        url: format!("https://archiveofourown.org{}", url),
                        date_posted,
                        author: None,
                    }))
                })
                .collect()
        } else {
            vec![{
                let posted_on = main_page
                    .find(predicate::Name("dd").and(predicate::Class("published")))
                    .next()
                    .ok_or(ArchiveError::PageError(format!(
                        "AO3: Could not find published date (dd.published) for story at {}",
                        source.to_url()
                    )))?
                    .text();
                let date_posted = date_string_to_datetime(posted_on)?;
                let text = get_chapter_text(&chapters, &url)?;
                Ok(Content::Chapter(Chapter {
                    id: format!("{}:", source.to_id()),
                    name: name.clone(),
                    description: None,
                    text: ChapterText::Hydrated(text),
                    url: source.to_url(),
                    date_posted,
                    author: None,
                }))
            }]
        };

        if chapters.iter().find(|c| c.is_err()).is_some() {
            return Err(chapters
                .into_iter()
                .find(|c| c.is_err())
                .unwrap()
                .unwrap_err());
        }

        let chapters = chapters
            .into_iter()
            .map(|c| c.expect("If there was an error we would have returned already."))
            .collect();

        Ok(Story {
            name: name.trim().to_owned(),
            authors: AuthorList::new(author),
            description: description.map(|d: String| d.trim().to_owned()),
            url,
            tags,
            chapters,
            source,
            completed,
        })
    }

    async fn fill_skeleton(&self, skeleton: Story) -> Result<Story> {
        Ok(skeleton)
    }

    async fn get_story(&self, source: StorySource) -> Result<Story> {
        self.get_skeleton(source).await
    }
}

fn get_chapter_id(chapter: &Node, source: &StorySource) -> Result<String> {
    let href = get_chapter_url(chapter, source)?;

    Ok(CHAPTER_REGEX
        .1
        .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
        .captures(&href)
        .unwrap()
        .get(1)
        .ok_or(ArchiveError::PageError(format!(
            "AO3: Could not find chapter id in chapter link {} for story at {}",
            href,
            source.to_url()
        )))?
        .as_str()
        .to_owned())
}

fn get_chapter_name(chapter: &Node, source: &StorySource) -> Result<String> {
    let full_title = chapter
        .descendants()
        .find(|n| n.is(predicate::Class("title")))
        .ok_or(ArchiveError::PageError(format!(
            "AO3: Could not find chapter name (.title) for a chapter in story at {}",
            source.to_url()
        )))?
        .text()
        .trim()
        .to_owned();
    Ok(full_title.splitn(2, ':').nth(1).or(full_title.splitn(2, ':').next()).ok_or(ArchiveError::PageError(format!("Expected chapter title to look like \"Chapter <num>\" or \"Chapter <num>: <name>\" but got {} for story at {}", full_title, source.to_url())))?.to_string())
}

fn get_chapter_url(chapter: &Node, source: &StorySource) -> Result<String> {
    Ok(chapter
        .descendants()
        .find(|n| n.is(predicate::Class("title").child(predicate::Attr("href", ()))))
        .ok_or(ArchiveError::PageError(format!(
            "AO3: Could not find chapter link (.title [href]) for a chapter in story at {}",
            source.to_url()
        )))?
        .attr("href")
        .expect("Node should have href guaranteed by above is()")
        .to_owned())
}

fn get_chapter_date_posted(
    navigate: &Document,
    href: &str,
    source: &StorySource,
) -> Result<DateTime<FixedOffset>> {
    let posted_on = navigate
        .find(predicate::Attr("href", href))
        .next()
        .ok_or(ArchiveError::PageError(format!("AO3: Could not find a link which matches \"{}\" on the navigation page for story at {}/navigate", href, source.to_url())))?
        .parent()
        .expect("Found node is not root")
        .children()
        .find_map(|c| {
            if c.is(predicate::Class("datetime")) {
                Some(c.text())
            } else {
                None
            }
        })
        .ok_or(ArchiveError::PageError(format!("AO3: Could not find a datetime span for the link matching \"{}\" on the navigation page for story at {}/nagivate", href, source.to_url())))?;
    date_string_to_datetime(posted_on)
}

fn date_string_to_datetime(date: String) -> Result<DateTime<FixedOffset>> {
    let posted_on = date.replace('(', "").replace(')', "");
    let date_posted = posted_on.trim();
    let timezone = FixedOffset::west(Local::now().offset().utc_minus_local());
    Ok(timezone
        .from_local_datetime(&NaiveDate::parse_from_str(date_posted, "%F")?.and_hms(3, 0, 0))
        .earliest()
        .ok_or(ArchiveError::PageError(format!(
            "AO3: Could not convert date string {} to a date",
            date
        )))?)
}

fn get_chapter_text(chapter: &Node, chapter_url: &String) -> Result<String> {
    let top_notes = chapter
        .children()
        .find(|c| c.is(predicate::Attr("id", "notes")));
    let bottom_notes = chapter
        .children()
        .find(|c| c.is(predicate::Class("end").and(predicate::Class("notes"))));
    let chapter_text = chapter
        .children()
        .find(|c| c.is(predicate::Class("userstuff")));

    Ok(format!(
        "{}{}{}",
        top_notes.map(|n| n.inner_html()).unwrap_or_default(),
        chapter_text
            .ok_or(ArchiveError::PageError(format!("AO3: Can't find text area ([id=\"chapters\"] > .userstuff) for chapter with URL {}", chapter_url)))?
            .children()
            .filter(|node| !node.is(predicate::Attr("id", "work")))
            .map(|node| node.html())
            .collect::<String>(),
        bottom_notes.map(|n| n.inner_html()).unwrap_or_default()
    ))
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

fn get_completed(document: &Document, source: &StorySource) -> Completed {
    document.find(
        predicate::Class("stats").child(predicate::Name("dt").and(predicate::Class("status"))))
        .next()
        .map(|node| match node.text().trim().to_lowercase().as_ref() {
            "updated:" => Completed::Incomplete,
            "completed:" => Completed::Complete,
            _ => {
                println!("Encountered unexpected value {} in story status tag (.stats > dt.status) for story at {}", node.text().trim().to_lowercase(), source.to_url());
                Completed::Unknown
            },
        })
        .unwrap_or(Completed::Complete) // If there is no "status" stat it's a oneshot and thus complete.
}
