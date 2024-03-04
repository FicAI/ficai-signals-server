use eyre::WrapErr;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub source: String,
}

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
    base_url: Url,
}

impl Client {
    pub fn new(client: reqwest::Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    pub async fn meta(&self, url: &str) -> eyre::Result<Option<Meta>> {
        Ok(Some(
            self.client
                .get(
                    self.base_url
                        .join("/api/v0/meta")
                        .wrap_err("failed to parse meta_url path")?,
                )
                .query(&[("q", url)])
                .send()
                .await
                .wrap_err("failed to send request")?
                .json()
                .await
                .wrap_err("failed to fetch body")?,
        ))
    }
}
