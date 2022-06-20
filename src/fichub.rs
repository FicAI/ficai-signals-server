use eyre::WrapErr;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Meta {
    id: String,
    source: String,
    title: String,
    author: String,
    chapters: i64,
    words: i64,
    description: String,
    created: String,
    updated: String,
}

pub async fn meta(client: reqwest::Client, url: &str) -> eyre::Result<Option<Meta>> {
    Ok(Some(
        client
            .get("https://fichub.net/api/v0/meta")
            .query(&[("q", url)])
            .send()
            .await
            .wrap_err("failed to send request")?
            .json()
            .await
            .wrap_err("failed to fetch body")?,
    ))
}
