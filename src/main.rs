use std::net::SocketAddr;
use std::sync::Arc;

use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use futures::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::Row as _;
use warp::{Filter as _, Reply};

use crate::httputil::recover_custom;
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
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> eyre::Result<()> {
    // todo: error handling
    let cfg = envy::prefixed("FICAI_").from_env::<Config>()
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

    let pepper = Arc::new(
        base64ct::Base64Unpadded::decode_vec(&cfg.pwd_pepper)
            .wrap_err("pepper is not valid base64")?
    );

    let domain = Arc::new(cfg.domain);

    let create_user = warp::path!("v1" / "accounts")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::CreateUserQ>())
        .and_then({
            let pool = pool.clone();
            let pepper = pepper.clone();
            let domain = domain.clone();
            move |q| crate::usermgmt::create_user(q, pool.clone(), pepper.clone(), domain.clone())
        });
    let log_in = warp::path!("v1" / "sessions")
        .and(warp::post())
        .and(warp::body::json::<crate::usermgmt::LogInQ>())
        .and_then({
            let pool = pool.clone();
            let pepper = pepper.clone();
            let domain = domain.clone();
            move |q| crate::usermgmt::log_in(q, pool.clone(), pepper.clone(), domain.clone())
        });

    let get = warp::path!("v1" / "signals")
        .and(warp::get())
        .and(authenticate(pool.clone()))
        .and(warp::query::<GetQueryParams>())
        .then({
            let pool = pool.clone();
            move |uid, q: GetQueryParams| get(uid, q.url, pool.clone())
        });
    let patch = warp::path!("v1" / "signals")
        .and(warp::patch())
        .and(authenticate(pool.clone()))
        .and(warp::body::json::<PatchQuery>())
        .then({
            let pool = pool.clone();
            move |uid, q: PatchQuery| patch(uid, q, pool.clone())
        });

    // todo: graceful shutdown
    warp::serve(
        create_user
            .or(log_in)
            .or(get)
            .or(patch)
            .recover(recover_custom)
    ).run(cfg.listen).await;

    Ok(())
}


#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GetQueryParams {
    url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TagInfo {
    tag: String,
    signal: Option<bool>,
    signals_for: i64,
    signals_against: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Tags {
    tags: Vec<TagInfo>,
}

async fn get(uid: i64, url: String, pool: DB) -> http::Response<hyper::Body> {
    let mut rows = sqlx::query("
select
	tag,
	sum(case when signal then 1 else 0 end) as total_for,
    sum(case when signal then 0 else 1 end) as total_against,
    bool_or(signal) filter (where user_id = $1) as my_signal
from signal
where url = $2
group by tag
")
        .bind(uid)
        .bind(url)
        .fetch(&pool);

    let mut tags = Vec::new();
    while let Some(row) = rows.try_next().await.unwrap() {
        let tag_info = TagInfo {
            tag: row.try_get("tag").unwrap(),
            signals_for: row.try_get("total_for").unwrap(),
            signals_against: row.try_get("total_against").unwrap(),
            signal: row.try_get("my_signal").unwrap()
        };
        tags.push(tag_info);
    }

    let tags = Tags { tags };
    warp::reply::json(&tags).into_response()
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

async fn patch(uid: i64, q: PatchQuery, pool: DB) -> impl Reply {
    // todo: sane error handling

    for tag in q.add {
        println!("add {}", &tag);
        sqlx::query("
insert into signal (user_id, url, tag, signal)
values ($1, $2, $3, $4)
on conflict (user_id, url, tag) do update set signal = $4
        ")
            .bind(uid)
            .bind(&q.url)
            .bind(tag)
            .bind(true)
            .execute(&pool)
            .await
            .unwrap();
    }

    for tag in q.rm {
        println!("rm {}", &tag);
        sqlx::query("
insert into signal (user_id, url, tag, signal)
values ($1, $2, $3, $4)
on conflict (user_id, url, tag) do update set signal = $4
        ")
            .bind(uid)
            .bind(&q.url)
            .bind(tag)
            .bind(false)
            .execute(&pool)
            .await
            .unwrap();
    }

    for tag in q.erase {
        println!("erase {}", &tag);
        sqlx::query("delete from signal where user_id = $1 and url = $2 and tag = $3")
            .bind(uid)
            .bind(&q.url)
            .bind(tag)
            .execute(&pool)
            .await
            .unwrap();
    }

    println!();

    warp::reply::reply()
}
