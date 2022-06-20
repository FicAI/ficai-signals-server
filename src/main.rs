use std::net::SocketAddr;

use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use warp::{Filter as _, Reply};

use crate::httputil::{recover_custom, Empty, Error};
use crate::usermgmt::{authenticate, optional_authenticate, UserSession};

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
    bex_current_version: String,
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
    let bex_current_version: &'static str = Box::leak(cfg.bex_current_version.into_boxed_str());

    let create_user = warp::path!("v1" / "accounts")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::CreateUserQ>())
        .and_then({
            let pool = pool.clone();
            move |q| crate::usermgmt::create_user(q, pool.clone(), pepper, domain, beta_key)
        });
    let log_in = warp::path!("v1" / "sessions")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::LogInQ>())
        .and_then({
            let pool = pool.clone();
            move |q| crate::usermgmt::log_in(q, pool.clone(), pepper, domain)
        });
    let log_out = warp::path!("v1" / "sessions")
        .and(warp::delete())
        .and(authenticate(pool.clone()))
        .and_then({
            let pool = pool.clone();
            move |user| crate::usermgmt::log_out(user, pool.clone(), domain)
        });
    let get_session_user = warp::path!("v1" / "sessions")
        .and(warp::get())
        .and(authenticate(pool.clone()))
        .and_then(crate::usermgmt::get_session_user);

    let get_signals = warp::path!("v1" / "signals")
        .and(warp::get())
        .and(optional_authenticate(pool.clone()))
        .and(warp::query::<GetQueryParams>())
        .then({
            let pool = pool.clone();
            move |user, q: GetQueryParams| get_signals(user, q.url, pool.clone())
        });
    let patch = warp::path!("v1" / "signals")
        .and(warp::patch())
        .and(authenticate(pool.clone()))
        .and(warp::body::json::<PatchQuery>())
        .then({
            let pool = pool.clone();
            move |user, q: PatchQuery| patch(user, q, pool.clone())
        });

    let get_urls = warp::path!("v1" / "urls").and(warp::get()).then({
        let pool = pool.clone();
        move || get_urls(pool.clone())
    });
    let get_tags = warp::path!("v1" / "tags")
        .and(warp::get())
        .and(warp::query::<GetTagsQ>())
        .then({
            let pool = pool.clone();
            move |q| get_tags(q, pool.clone())
        });

    let get_bex_version = warp::path!("v1" / "bex" / "versions" / String)
        .and(warp::get())
        .then({
            let pool = pool.clone();
            move |v| get_bex_version(v, pool.clone(), bex_current_version)
        });

    // todo: graceful shutdown
    warp::serve(
        create_user
            .or(log_in)
            .or(log_out)
            .or(get_session_user)
            .or(get_signals)
            .or(patch)
            .or(get_urls)
            .or(get_tags)
            .or(get_bex_version)
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
    pub async fn get(pool: &DB, uid: Option<i64>, url: String) -> eyre::Result<Vec<TagInfo>> {
        Ok(sqlx::query_as::<_, TagInfo>(
            "
select
    tag,
    sum(case when signal then 1 else 0 end) as signals_for,
    sum(case when signal then 0 else 1 end) as signals_against,
    bool_or(signal) filter (where user_id = $1) as signal
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

async fn get_signals(
    user: Option<UserSession>,
    url: String,
    pool: DB,
) -> http::Response<hyper::Body> {
    json_or_error(
        TagInfo::get(&pool, user.map(|u| u.id), url)
            .await
            .map(|tags| Tags { tags })
            .wrap_err("failed to get tags"),
    )
}

struct Signal;

impl Signal {
    pub async fn set(pool: &DB, uid: i64, url: &str, tag: &str, signal: bool) -> eyre::Result<()> {
        sqlx::query(
            "
insert into signal (user_id, url, tag, signal)
values ($1, $2, $3, $4)
on conflict (user_id, url, tag) do update set signal = $4
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

    pub async fn erase(pool: &DB, uid: i64, url: &str, tag: &str) -> eyre::Result<()> {
        sqlx::query("delete from signal where user_id = $1 and url = $2 and tag = $3")
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

async fn patch_signals(pool: &DB, uid: i64, q: PatchQuery) -> eyre::Result<()> {
    for tag in q.add {
        println!("add {}", &tag);
        Signal::set(pool, uid, &q.url, &tag, true)
            .await
            .wrap_err("failed to add signal")?
    }

    for tag in q.rm {
        println!("rm {}", &tag);
        Signal::set(pool, uid, &q.url, &tag, false)
            .await
            .wrap_err("failed to rm signal")?
    }

    for tag in q.erase {
        println!("erase {}", &tag);
        Signal::erase(pool, uid, &q.url, &tag)
            .await
            .wrap_err("failed to erase signal")?
    }

    println!();
    Ok(())
}

async fn patch(user: UserSession, q: PatchQuery, pool: DB) -> http::Response<hyper::Body> {
    json_or_error(
        patch_signals(&pool, user.id, q)
            .await
            .map(|_| Empty {})
            .wrap_err("failed to patch signals"),
    )
}

fn json_or_error<T: Serialize, E: std::fmt::Display + std::fmt::Debug>(
    val: Result<T, E>,
) -> http::Response<hyper::Body> {
    match val {
        Ok(val) => warp::reply::json(&val).into_response(),
        Err(e) => {
            eprintln!("error: {:#?}", e);
            warp::reply::with_status(
                warp::reply::json(&Error {
                    code: "internal_server_error".to_string(),
                    message: format!("{:#}", e),
                }),
                http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct URLs {
    urls: Vec<String>,
}

async fn get_urls(pool: DB) -> http::Response<hyper::Body> {
    json_or_error(
        sqlx::query_scalar::<_, String>("select distinct url from signal")
            .fetch_all(&pool)
            .await
            .map(|urls| URLs { urls })
            .wrap_err("failed to query urls"),
    )
}

#[derive(Deserialize, Debug)]
struct GetTagsQ {
    q: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JustTags {
    tags: Vec<String>,
}

async fn get_tags(q: GetTagsQ, pool: DB) -> http::Response<hyper::Body> {
    // todo: something better than levenshtein, this is pretty bad
    json_or_error(
        sqlx::query_scalar::<_, String>(
            "
select tag
from signal
group by tag
order by
    (
        levenshtein(tag, $1) * 1.0
        / greatest(octet_length(tag), octet_length($1))
    ) asc,
    count(1) desc,
    tag asc
limit $2",
        )
        .bind(&q.q)
        .bind(&q.limit.unwrap_or(1000))
        .fetch_all(&pool)
        .await
        .map(|tags| JustTags { tags })
        .wrap_err("failed to query tags"),
    )
}

#[derive(Serialize)]
struct Bex {
    retired: bool,
    current_version: String,
}

async fn get_bex_version(
    v: String,
    _pool: DB,
    bex_current_version: &str,
) -> http::Response<hyper::Body> {
    warp::reply::json(&Bex {
        retired: v == "v0.0.0",
        current_version: bex_current_version.to_string(),
    })
    .into_response()
}
