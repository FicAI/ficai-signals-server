use std::net::SocketAddr;

use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use warp::{Filter as _, Reply};

use crate::httputil::{recover_custom, Empty, Error};
use crate::signal::{Signal, Signals};
use crate::usermgmt::authenticate;

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

    let get_signals = warp::path!("v1" / "signals")
        .and(warp::get())
        .and(authenticate.clone())
        .and(warp::query::<GetQueryParams>())
        .and(pool.clone())
        .then(get_signals)
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
            .or(get_signals)
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

async fn get_signals(uid: i64, q: GetQueryParams, pool: DB) -> eyre::Result<Signals> {
    Signals::get(uid, q.url, &pool)
        .await
        .wrap_err("failed to get signals")
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
