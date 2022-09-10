use std::net::SocketAddr;

use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use warp::{Filter as _, Reply};

use crate::httputil::{recover_custom, Empty, Error};
use crate::usermgmt::authenticate;

mod httputil;
mod usermgmt;

pub type DB = sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Config {
    listen: SocketAddr,
    db_host: String,
    db_port: u16,
    db_username: String,
    db_password: String,
    db_database: String,
    pwd_pepper: String,
    domain: String,
    beta_key: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> eyre::Result<()> {
    // todo: error handling
    let cfg = envy::prefixed("FICAI_")
        .from_env::<Config>()
        .wrap_err("bad configuration")?;

    let conn_opt = PgConnectOptions::new()
        .host(&cfg.db_host)
        .port(cfg.db_port)
        .username(&cfg.db_username)
        .password(&cfg.db_password)
        .database(&cfg.db_database)
        // todo: sqlx doesn't support target_session_attrs (at time of writing), find another way
        // .options([("target_session_attrs", "read-write")])
        ;
    // todo: error handling
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(conn_opt)
        .await
        .map_err(|e| eyre!("failed to connect to database: {:?}", e))?;

    let pepper: &'static [u8] = Box::leak(
        base64ct::Base64Unpadded::decode_vec(&cfg.pwd_pepper)
            .wrap_err("pepper is not valid base64")?
            .into_boxed_slice(),
    );

    let domain: &'static str = Box::leak(cfg.domain.into_boxed_str());
    let beta_key: &'static str = Box::leak(cfg.beta_key.into_boxed_str());

    let authenticate = authenticate(pool.clone());
    let pool = warp::any().map(move || pool.clone());

    let create_account = warp::path!("v1" / "accounts")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::CreateAccountQ>())
        .and(pool.clone())
        .and_then(move |q, pool| {
            crate::usermgmt::create_account(q, pool, pepper, domain, beta_key)
        });
    let create_session = warp::path!("v1" / "sessions")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::CreateSessionQ>())
        .and(pool.clone())
        .and_then(move |q, pool| crate::usermgmt::create_session(q, pool, pepper, domain));

    let get = warp::path!("v1" / "signals")
        .and(warp::get())
        .and(authenticate.clone())
        .and(warp::query::<GetQueryParams>())
        .and(pool.clone())
        .then(Tags::get)
        .then(reply_json);
    let patch = warp::path!("v1" / "signals")
        .and(warp::patch())
        .and(authenticate.clone())
        .and(warp::body::json::<PatchQuery>())
        .and(pool.clone())
        .then(patch_signals)
        .then(reply_json);

    // todo: graceful shutdown
    warp::serve(
        create_account
            .or(create_session)
            .or(get)
            .or(patch)
            .recover(recover_custom),
    )
    .run(cfg.listen)
    .await;

    Ok(())
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GetQueryParams {
    url: String,
}

#[derive(Serialize, Debug, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct TagInfo {
    tag: String,
    signal: Option<bool>,
    signals_for: i64,
    signals_against: i64,
}

impl TagInfo {
    pub async fn get(uid: i64, url: String, pool: &DB) -> eyre::Result<Vec<TagInfo>> {
        Ok(sqlx::query_as::<_, TagInfo>(
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
        .await?)
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Tags {
    tags: Vec<TagInfo>,
}

impl Tags {
    async fn get(uid: i64, q: GetQueryParams, pool: DB) -> eyre::Result<Self> {
        Ok(Self {
            tags: TagInfo::get(uid, q.url, &pool)
                .await
                .wrap_err("failed to get tags")?,
        })
    }
}

struct Signal;

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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PatchQuery {
    url: String,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    rm: Vec<String>,
    #[serde(default)]
    erase: Vec<String>,
}

async fn patch_signals(uid: i64, q: PatchQuery, pool: DB) -> eyre::Result<Empty> {
    for tag in q.add {
        println!("add {}", &tag);
        Signal::set(uid, &q.url, &tag, true, &pool)
            .await
            .wrap_err("failed to add signal")?
    }

    for tag in q.rm {
        println!("rm {}", &tag);
        Signal::set(uid, &q.url, &tag, false, &pool)
            .await
            .wrap_err("failed to rm signal")?
    }

    for tag in q.erase {
        println!("erase {}", &tag);
        Signal::erase(uid, &q.url, &tag, &pool)
            .await
            .wrap_err("failed to erase signal")?
    }

    println!();
    Ok(Empty {})
}

async fn reply_json<T: Serialize, E: std::fmt::Display + std::fmt::Debug>(
    val: Result<T, E>,
) -> http::Response<hyper::Body> {
    match val {
        Ok(val) => warp::reply::json(&val).into_response(),
        Err(e) => {
            eprintln!("error: {:#?}", e);
            warp::reply::with_status(
                warp::reply::json(&Error {
                    message: format!("{:#}", e),
                }),
                http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    }
}
