use async_trait::async_trait;
use chrono::DateTime;
use futures::future::join_all;
use regex::Regex;
use select::{
    document::Document,
    predicate::{self, Predicate},
};

use crate::{
    client::get,
    error::ArchiveError,
    parser::Parser,
    structs::{Author, AuthorList, Chapter, ChapterText, Completed, Content, Story, StorySource},
    Result,
};

static CHAPTER_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"#post-(\d+)", once_cell::sync::OnceCell::new());
static AUTHOR_REGEX: (&str, once_cell::sync::OnceCell<regex::Regex>) =
    (r"/members/(?:.+\.)?(\d+)", once_cell::sync::OnceCell::new());

pub(crate) struct XenforoParser;

#[async_trait]
impl Parser for XenforoParser {
    async fn get_skeleton(&self, source: StorySource) -> Result<Story> {
        let main_page = get(&format!("{}/threadmarks", source.to_url()))
            .await?
            .text()
            .await?;
        let document = Document::from_read(main_page.as_bytes())?;

        let name = document
            .find(predicate::Class("threadmarkListingHeader-name"))

            .next()
            .ok_or(ArchiveError::PageError(format!(
                "Xenforo: Could not find title (.threadmarkListingHeader-name) for story at {}/threadmarks",
                source.to_url()
            )))?
            .children()
            .filter(|c| c.name().is_none()) // Text nodes have None for name()
            .next()
            .ok_or(ArchiveError::PageError(format!("Xenforo: Could not find text in title (.threadmarkListingHeader-name) for story at {}/threadmarks", source. to_url())))?
            .text()
            .replace(" - Threadmarks", "")
            .trim()
            .to_owned();
        let authors: Vec<Result<Author>> = document.find(predicate::Class("username"))
            .map(|node| {
                let author_url = node
                    .attr("href")
                    .ok_or(
                        ArchiveError::PageError(
                            format!(
                                "Xenforo: Could not find user profile link (.username[href]) for user {} in story at {}/threadmarks",
                                node.text().trim().to_owned(),
                                source.to_url())))?;
                let author_id = AUTHOR_REGEX
                    .1
                    .get_or_init(|| Regex::new(AUTHOR_REGEX.0).unwrap())
                    .captures(author_url)
                    .unwrap()
                    .get(1)
                    .ok_or(ArchiveError::PageError(format!(
                        "Xenforo: Could not find author id in author link {} for story at {}/threadmarks",
                        author_url,
                        source.to_url()
                    )))?
                    .as_str();
                Ok(Author {
                    id: format!("{}:{}", source.prefix(), author_id),
                    name: node.text().trim().to_owned(),
                })
            })
            .collect();
        if authors.iter().find(|res| res.is_err()).is_some() {
            return Err(authors
                .into_iter()
                .find(|res| res.is_err())
                .unwrap()
                .unwrap_err());
        }
        let authors: Vec<Author> = authors.into_iter().map(|res| res.unwrap()).collect();

        let description = None;

        let url = source.to_url();

        let tags = Vec::new();

        let completed = document
            .find(predicate::Class("pairs--rows"))
            .find(|node| {
                node.children()
                    .find(|c| {
                        c.is(predicate::Name("dt"))
                            && c.text().trim().to_lowercase() == "index progress"
                    })
                    .is_some()
            })
            .map(|node| {
                match node
                    .children()
                    .find(|c| c.is(predicate::Name("dd")))
                    .map(|c| c.text())
                    .unwrap_or("not found".to_owned())
                    .as_ref()
                {
                    "Complete" => Completed::Complete,
                    "Ongoing" => Completed::Incomplete,
                    _ => Completed::Unknown,
                }
            })
            .unwrap_or(Completed::Unknown);

        let chapters: Vec<Result<Content>> = document.find(predicate::Class("structItem--threadmark"))
            .map(|node| {
                let chapter_info = node.descendants().find(|node| node.is(predicate::Class("structItem-title"))).ok_or(
                    ArchiveError::PageError(format!("Xenforo: Could not find threadmark title container (.structItem-title) for a threadmark for story at {}/threadmarks", source.to_url())))?
                    .descendants()
                    .find(|node| node.is(predicate::Name("a").and(predicate::Attr("href", ()))))
                    .ok_or(ArchiveError::PageError(format!("Xenforo: Could not find threadmark link (.structItem-title a) for a threadmark for story at {}/threadmarks", source.to_url())))?;
                let chapter_url = chapter_info.attr("href").expect("Should not fail due to filter above.");
                let chapter_id = CHAPTER_REGEX
                    .1
                    .get_or_init(|| Regex::new(CHAPTER_REGEX.0).unwrap())
                    .captures(chapter_url)
                    .unwrap()
                    .get(1)
                    .ok_or(ArchiveError::PageError(format!(
                        "Xenforo: Could not find chapter id in chapter link {} for story at {}/threadmarks",
                        chapter_url,
                        source.to_url()
                    )))?
                    .as_str()
                    .to_owned();
                let chapter_url = format!("{}/posts/{}", source.to_base_url(), chapter_id);
                let chapter_title = chapter_info.text().trim().to_string();

                let time_string = node.descendants().find(|node| node.is(predicate::Name("time").and(predicate::Attr("datetime", ())))).ok_or(
                    ArchiveError::PageError(format!("Xenforo: Could not find threadmark date posted (structItem--threadmark time[datetime]) for a threadmark for story at {}/threadmarks", source.to_url())))?.attr("datetime").expect("Should not fail due to filter above.");
                let date_posted = DateTime::parse_from_str(time_string, "%FT%T%z").unwrap_or_else(|_| {
                    panic!(
                        "Chapter posted-on date ({}) did not conform to rfc3339",
                        time_string
                    )
                });

                let author_name = node.attr("data-content-author").ok_or(ArchiveError::PageError(format!("Xenforo: Could not find author name (structItem--threadmark.data-content-author for a threadmark for story at {}/threadmarks", source.to_url())))?;

                Ok(Content::Chapter(Chapter {
                    id: format!("{}:{}", source.to_id(), chapter_id),
                    name: chapter_title,
                    description: None,
                    text: ChapterText::Dehydrated,
                    url: chapter_url,
                    date_posted,
                    author: Some(authors.iter().find(|a| a.name == author_name).ok_or(ArchiveError::PageError(format!("Xenforo: Could not find an author (.username) matching {} for story at {}/threadmarks", author_name, source.to_url())))?.clone()),
                }))
            })
            .collect();
        if chapters.iter().find(|r| r.is_err()).is_some() {
            return Err(chapters
                .into_iter()
                .find(Result::is_err)
                .unwrap()
                .unwrap_err());
        }

        Ok(Story {
            name,
            authors: AuthorList::from_list(authors),
            description,
            url,
            tags,
            chapters: chapters.into_iter().map(Result::unwrap).collect(),
            source,
            completed,
        })
    }

    async fn fill_skeleton(&self, mut skeleton: Story) -> Result<Story> {
        let page_list: Vec<String> = {
            let first_page = get(format!("{}/reader", skeleton.source.to_url()).as_ref())
                .await?
                .text()
                .await?;
            let first_page = Document::from_read(first_page.as_bytes())?;

            let last_page = first_page.find(predicate::Class("pageNav-main")).next()
                .map(|node| match node.descendants()
                        .filter(|d| d.is(predicate::Name("a").and(predicate::Attr("href", ()))))
                        .last()
                    {
                        Some(last_page) => usize::from_str_radix(&last_page.text(), 10).map_err(ArchiveError::from),
                        None => Err(ArchiveError::PageError(format!(
                            "Xenforo: Could not find pageNav (.pageNav-main a[href]) for story at {}/reader",
                            skeleton.source.to_url()
                        ))),
                    })
                .unwrap_or(Ok(1))?;
            (1..=last_page)
                .map(|num| format!("{}/reader/page-{}", skeleton.source.to_url(), num))
                .collect()
        };
        let page_list = page_list
            .into_iter()
            .map(|p| async move { Ok(get(p.as_ref()).await?.text().await?) });
        let pages = join_all(page_list).await;
        let pages = extract_error(pages)?
            .into_iter()
            .map(|text| Document::from_read(text.as_bytes()).map_err(ArchiveError::from))
            .collect();
        let pages = extract_error(pages)?;
        let maybe_text: Vec<Result<(&mut Chapter, ChapterText)>> = skeleton
            .chapters
            .iter_mut()
            .filter_map(|content| match content {
                Content::Section(_) => None,
                Content::Chapter(chap) => Some(chap),
            })
            .map(|chap| {
                let chapter_id = chap.chapter_id();
                let selector = format!("js-post-{}", chapter_id);
                let elem = pages.iter().find(|page| page.find(predicate::Attr("id", selector.as_ref())).next().is_some()).ok_or(
                    ArchiveError::PageError(format!("Xenforo: could not find a post for chapter with id {chapter_id} (.js-post-{chapter_id}) on any page for story at {}/reader", skeleton.source.to_url())))?;
                let content = elem.find(predicate::Attr("id", selector.as_ref())).next().unwrap().descendants().find(|d| d.is(predicate::Class("bbWrapper"))).ok_or(
                    ArchiveError::PageError(format!("Xenforo: could not find text content for post with id {chapter_id} (.js-post-{chapter_id} .bbWrapper) on any page for story at {}/reader", skeleton.source.to_url())))?;
                Ok((chap, ChapterText::Hydrated(content.inner_html())))
            })
            .collect();

        let maybe_text = extract_error(maybe_text)?;
        maybe_text.into_iter().for_each(|(chap, text)| {
            chap.text = text;
        });

        Ok(skeleton)
    }

    async fn get_story(&self, source: StorySource) -> Result<Story> {
        let story = self.get_skeleton(source).await?;
        self.fill_skeleton(story).await
    }
}
fn extract_error<O: core::fmt::Debug>(list: Vec<Result<O>>) -> Result<Vec<O>> {
    if list.iter().find(|i| i.is_err()).is_some() {
        Err(list.into_iter().find(Result::is_err).unwrap().unwrap_err())
    } else {
        Ok(list.into_iter().map(Result::unwrap).collect())
    }
}
