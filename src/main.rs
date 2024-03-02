use std::net::SocketAddr;

use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use warp::{Filter as _, Reply};

use crate::httputil::{recover_custom, Empty, Error};
use crate::signal::{Signal, Signals};
use crate::usermgmt::{authenticate, optional_authenticate, AccountSession};

mod httputil;
mod signal;
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
    bex_latest_version: String,
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
    let bex_latest_version: &'static str = Box::leak(cfg.bex_latest_version.into_boxed_str());

    let authenticate = authenticate(pool.clone());
    let optional_authenticate = optional_authenticate(pool.clone());
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
    let get_session_account = warp::path!("v1" / "sessions")
        .and(warp::get())
        .and(authenticate.clone())
        .and_then(crate::usermgmt::get_session_account);
    let delete_session = warp::path!("v1" / "sessions")
        .and(warp::delete())
        .and(authenticate.clone())
        .and(pool.clone())
        .and_then(move |session, pool| crate::usermgmt::delete_session(session, pool, domain));

    let get_signals = warp::path!("v1" / "signals")
        .and(warp::get())
        .and(optional_authenticate.clone())
        .and(warp::query::<GetSignalsQ>())
        .and(pool.clone())
        .then(get_signals)
        .then(reply_json);
    let patch_signals = warp::path!("v1" / "signals")
        .and(warp::patch())
        .and(authenticate.clone())
        .and(warp::body::json::<PatchSignalsQ>())
        .and(pool.clone())
        .then(patch_signals)
        .then(reply_json);

    let get_tags = warp::path!("v1" / "tags")
        .and(warp::get())
        .and(warp::query::<GetTagsQ>())
        .and(pool.clone())
        .then(get_tags)
        .then(reply_json);

    let get_bex_version = warp::path!("v1" / "bex" / "versions" / String)
        .and(warp::get())
        .and(pool.clone())
        .then(|v, pool| get_bex_version(v, pool, bex_latest_version))
        .then(reply_json);

    // todo: graceful shutdown
    warp::serve(
        create_account
            .or(create_session)
            .or(get_session_account)
            .or(delete_session)
            .or(get_signals)
            .or(patch_signals)
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
struct GetSignalsQ {
    url: String,
}

async fn get_signals(
    account: Option<AccountSession>,
    q: GetSignalsQ,
    pool: DB,
) -> eyre::Result<Signals> {
    Signals::get(account.map(|a| a.id), q.url, &pool)
        .await
        .wrap_err("failed to get signals")
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PatchSignalsQ {
    url: String,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    rm: Vec<String>,
    #[serde(default)]
    erase: Vec<String>,
}

async fn patch_signals(account: AccountSession, q: PatchSignalsQ, pool: DB) -> eyre::Result<Empty> {
    for tag in q.add {
        println!("add {}", &tag);
        Signal::set(account.id, &q.url, &tag, true, &pool)
            .await
            .wrap_err("failed to add signal")?
    }

    for tag in q.rm {
        println!("rm {}", &tag);
        Signal::set(account.id, &q.url, &tag, false, &pool)
            .await
            .wrap_err("failed to rm signal")?
    }

    for tag in q.erase {
        println!("erase {}", &tag);
        Signal::erase(account.id, &q.url, &tag, &pool)
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

#[derive(Deserialize, Debug)]
struct GetTagsQ {
    q: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Tags {
    tags: Vec<String>,
}

async fn get_tags(q: GetTagsQ, pool: DB) -> eyre::Result<Tags> {
    // todo: something better than levenshtein, this is pretty bad
    Ok(Tags {
        tags: sqlx::query_scalar::<_, String>(
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
limit $2
            ",
        )
        .bind(&q.q)
        .bind(q.limit.unwrap_or(1000))
        .fetch_all(&pool)
        .await
        .wrap_err("failed to query tags")?,
    })
}

#[derive(Serialize)]
struct Bex {
    retired: bool,
    latest_version: String,
}

async fn get_bex_version(v: String, _pool: DB, bex_latest_version: &str) -> eyre::Result<Bex> {
    Ok(Bex {
        retired: v == "v0.0.0",
        latest_version: bex_latest_version.to_string(),
    })
}
