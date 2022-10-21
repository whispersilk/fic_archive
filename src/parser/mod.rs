use async_trait::async_trait;
use html2md::parse_html;
use pandoc::{InputFormat, InputKind, OutputFormat, OutputKind, PandocOutput};
use reqwest::Client;

use crate::{
    error::ArchiveError,
    structs::{Story, StorySource, TextFormat},
};

pub mod ao3;
pub mod katalepsis;
pub mod royalroad;

#[async_trait]
pub trait Parser {
    fn get_client(&self) -> Client;
    async fn get_skeleton(
        &self,
        client: &Client,
        source: StorySource,
    ) -> Result<Story, ArchiveError>;
    async fn fill_skeleton(&self, client: &Client, skeleton: Story) -> Result<Story, ArchiveError>;
    async fn get_story(&self, source: StorySource) -> Result<Story, ArchiveError>;
}

fn convert_to_format(html: String, format: TextFormat) -> String {
    custom_convert_to_format(html, format, None)
}

fn custom_convert_to_format(
    html: String,
    format: TextFormat,
    custom_behavior: Option<Box<dyn Fn(String, TextFormat) -> String>>,
) -> String {
    let initial_text = match format {
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
    };
    match custom_behavior {
        Some(f) => f(initial_text, format),
        None => initial_text,
    }
}
