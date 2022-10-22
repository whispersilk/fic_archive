use reqwest::{Client, Response, StatusCode};
use serde::ser::Serialize;

use std::time::Duration;

use crate::error::ArchiveError;

static CLIENT: once_cell::sync::OnceCell<Client> = once_cell::sync::OnceCell::new();

pub async fn get(url: &str) -> Result<Response, ArchiveError> {
    let client: &Client =
        CLIENT.get_or_init(|| Client::builder().cookie_store(true).build().unwrap());
    let mut response = client.get(url).send().await?;
    loop {
        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                let base_url = &url[url.find("://").unwrap() + 3..];
                let base_url = &base_url[0..base_url.find("/").unwrap_or(base_url.len())];
                let time_to_wait: String = response.headers().get("retry-after").map_or_else(
                    || "60".to_owned(),
                    |v| {
                        v.to_str()
                            .map(|ok| ok.to_owned())
                            .unwrap_or("60".to_owned())
                    },
                );
                let time_to_wait = u64::from_str_radix(&time_to_wait, 10).expect(&format!(
                    "retry-after header {} is not a number",
                    time_to_wait
                ));
                println!(
                    "Too many requests to {}. Sleeping for {} seconds.",
                    base_url, time_to_wait
                );
                tokio::time::sleep(Duration::from_secs(time_to_wait)).await;
                response = client.get(url).send().await?;
            }
            _ => break Ok(response),
        }
    }
}

pub async fn get_with_query<T: Serialize + ?Sized>(
    url: &str,
    query: &T,
) -> Result<Response, ArchiveError> {
    let client: &Client =
        CLIENT.get_or_init(|| Client::builder().cookie_store(true).build().unwrap());
    let mut response = client.get(url).query(query).send().await?;
    loop {
        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                let base_url = &url[url.find("://").unwrap() + 3..];
                let base_url = &base_url[0..base_url.find("/").unwrap_or(base_url.len())];
                let time_to_wait: String = response.headers().get("retry-after").map_or_else(
                    || "60".to_owned(),
                    |v| {
                        v.to_str()
                            .map(|ok| ok.to_owned())
                            .unwrap_or("60".to_owned())
                    },
                );
                let time_to_wait = u64::from_str_radix(&time_to_wait, 10).expect(&format!(
                    "retry-after header {} is not a number",
                    time_to_wait
                ));
                println!(
                    "Too many requests to {}. Sleeping for {} seconds.",
                    base_url, time_to_wait
                );
                tokio::time::sleep(Duration::from_secs(time_to_wait)).await;
                response = client.get(url).query(query).send().await?;
            }
            _ => break Ok(response),
        }
    }
}
