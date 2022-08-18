use reqwest::Client;
use select::document::Document;
use select::predicate;
use tokio::io::{stdout, AsyncWriteExt as _};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let resp = client.get("https://katalepsis.net/table-of-contents/").send().await?;
    println!("Response: {}", resp.status());
    
    let document = resp.text().await?;
    let document = Document::from_read(document.as_bytes())?;
    let toc = document.find(predicate::Name("div")).into_selection().find(predicate::Class("entry-content")).find(predicate::Name("a"));
    for link in toc.iter() {
        stdout().write(format!("{} {}\n", link.text(), link.attr("href").unwrap()).as_bytes()).await?;
    }
    Ok(())
}
