use serde::Serialize;

use crate::DB;

#[derive(Serialize, Debug, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    tag: String,
    signal: Option<bool>,
    signals_for: i64,
    signals_against: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Signals {
    signals: Vec<Signal>,
}

impl Signal {
    pub async fn set(uid: i64, url: &str, tag: &str, signal: bool, pool: &DB) -> eyre::Result<()> {
        sqlx::query(
            "
insert into signal (account_id, url, tag, signal)
values ($1, $2, $3, $4)
on conflict (account_id, url, tag) do update set signal = $4
            ",
        )
        .bind(uid)
        .bind(url)
        .bind(tag)
        .bind(signal)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn erase(uid: i64, url: &str, tag: &str, pool: &DB) -> eyre::Result<()> {
        sqlx::query("delete from signal where account_id = $1 and url = $2 and tag = $3")
            .bind(uid)
            .bind(url)
            .bind(tag)
            .execute(pool)
            .await?;
        Ok(())
    }
}

impl Signals {
    pub async fn get(uid: i64, url: String, pool: &DB) -> eyre::Result<Self> {
        Ok(Self {
            signals: sqlx::query_as::<_, Signal>(
                "
select
    tag,
    sum(case when signal then 1 else 0 end) as signals_for,
    sum(case when signal then 0 else 1 end) as signals_against,
    bool_or(signal) filter (where account_id = $1) as signal
from signal
where url = $2
group by tag
    ",
            )
            .bind(uid)
            .bind(url)
            .fetch_all(pool)
            .await?,
        })
    }
}
