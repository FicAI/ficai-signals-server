use argon2::{Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _};
use base64ct::Encoding as _;
use eyre::{ensure, WrapErr};
use http::header::SET_COOKIE;
use http::{Response, StatusCode};
use hyper::Body;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sqlx::Row as _;
use warp::{Filter, Rejection, Reply};

use crate::httputil::{AccountAlreadyExists, BadRequest, Forbidden, InternalError};
use crate::DB;

const SESSION_COOKIE_NAME: &str = "FicAiSession";

const CONSTRAINT_VIOLATION_SQLSTATE: &str = "23505";

// https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-length
const SESSION_ID_BYTES: usize = 16;

fn create_kdf(pepper: &[u8]) -> Argon2 {
    use argon2::{Algorithm::Argon2id, Params, Version::V0x13};
    // https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id
    let params =
        Params::new(37 * 1024, 1, 1, Some(32)).expect("failed to assemble Argon2 parameters");
    Argon2::new_with_secret(pepper, Argon2id, V0x13, params).expect("failed to initialize Argon2")
}

struct Session(String);

impl Session {
    async fn create(uid: i64, db: &DB) -> eyre::Result<Self> {
        let mut session_id = [0u8; SESSION_ID_BYTES];
        let mut inserted = false;
        for _ in 0..3 {
            OsRng.fill_bytes(&mut session_id);
            let insert_result = sqlx::query("insert into session (id, account_id) values ($1, $2)")
                .bind(&session_id[..])
                .bind(uid)
                .execute(db)
                .await;
            match insert_result {
                Ok(_) => {
                    inserted = true;
                    break;
                }
                Err(sqlx::Error::Database(db_err))
                    if db_err.code() == Some(CONSTRAINT_VIOLATION_SQLSTATE.into()) =>
                {
                    continue
                }
                Err(e) => return Err(e).wrap_err("failed to insert new session"),
            }
        }
        ensure!(
            inserted,
            "failed to generate a new session id in 3 attempts"
        );
        Ok(Self(base64ct::Base64Unpadded::encode_string(&session_id)))
    }

    fn into_cookie(self, domain: &str) -> String {
        cookie::Cookie::build(SESSION_COOKIE_NAME, self.0)
            .domain(domain)
            .path("/")
            .secure(true)
            .http_only(true)
            .permanent()
            .finish()
            .to_string()
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountQ {
    email: String,
    password: String,
    beta_key: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    id: i64,
    email: String,
}

pub async fn create_account(
    q: CreateAccountQ,
    pool: DB,
    pepper: &[u8],
    domain: &str,
    beta_key: &str,
) -> Result<Response<Body>, Rejection> {
    if q.beta_key != beta_key {
        return Err(warp::reject::custom(BadRequest("invalid beta key".into())));
    }
    let hash = {
        let kdf = create_kdf(pepper);
        let salt = argon2::password_hash::SaltString::generate(OsRng);
        kdf.hash_password(q.password.as_bytes(), &salt)
            .expect("failed to hash password")
            .to_string()
    };
    let row =
        sqlx::query(r#"insert into account (email, password_hash) values ($1, $2) returning id"#)
            .bind(&q.email)
            .bind(hash)
            .fetch_one(&pool)
            .await;
    let uid: i64 = match row {
        Ok(row) => row.get("id"),
        Err(sqlx::Error::Database(db_err))
            if db_err.code() == Some(CONSTRAINT_VIOLATION_SQLSTATE.into()) =>
        {
            return Err(warp::reject::custom(AccountAlreadyExists))
        }
        Err(e) => {
            eprintln!("{:?}", e);
            return Err(warp::reject::custom(InternalError));
        }
    };

    let session_id_cookie = Session::create(uid, &pool)
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            warp::reject::custom(InternalError)
        })?
        .into_cookie(domain);
    Ok(warp::reply::with_header(
        warp::reply::with_status(
            warp::reply::json(&Account {
                id: uid,
                email: q.email,
            }),
            StatusCode::CREATED,
        ),
        SET_COOKIE,
        session_id_cookie,
    )
    .into_response())
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionQ {
    email: String,
    password: String,
}

pub async fn create_session(
    q: CreateSessionQ,
    db: DB,
    pepper: &[u8],
    domain: &str,
) -> Result<Response<Body>, Rejection> {
    let row = sqlx::query(r#"select id, password_hash from account where email = $1"#)
        .bind(&q.email)
        .fetch_optional(&db)
        .await;
    let (uid, db_hash_string): (i64, String) = match row {
        Ok(Some(row)) => (row.get("id"), row.get("password_hash")),
        Ok(None) => return Err(warp::reject::custom(Forbidden)),
        Err(e) => {
            eprintln!("{:?}", e);
            return Err(warp::reject::custom(InternalError));
        }
    };
    let db_hash =
        PasswordHash::new(&db_hash_string).map_err(|_| warp::reject::custom(InternalError))?;
    match create_kdf(pepper).verify_password(q.password.as_bytes(), &db_hash) {
        Ok(_) => {}
        Err(argon2::password_hash::Error::Password) => return Err(warp::reject::custom(Forbidden)),
        Err(e) => {
            eprintln!("{:?}", e);
            return Err(warp::reject::custom(Forbidden));
        }
    }
    let session_id_cookie = Session::create(uid, &db)
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            warp::reject::custom(InternalError)
        })?
        .into_cookie(domain);
    Ok(warp::reply::with_header(
        warp::reply::json(&Account {
            id: uid,
            email: q.email,
        }),
        SET_COOKIE,
        session_id_cookie,
    )
    .into_response())
}

pub fn authenticate(db: DB) -> impl Filter<Extract = (i64,), Error = Rejection> + Clone {
    warp::cookie::optional(SESSION_COOKIE_NAME).and_then(move |cookie: Option<String>| {
        let db = db.clone();
        async move {
            let cookie = match cookie {
                Some(cookie) => cookie,
                None => return Err(warp::reject::custom(Forbidden)),
            };
            let cookie = match base64ct::Base64Unpadded::decode_vec(&cookie) {
                Ok(cookie) => cookie,
                Err(_) => {
                    return Err(warp::reject::custom(BadRequest(
                        "invalid auth cookie".into(),
                    )))
                }
            };

            let row = sqlx::query("select account_id from session where id = $1")
                .bind(cookie)
                .fetch_one(&db)
                .await;
            match row {
                Ok(row) => Ok(row.get::<i64, _>("account_id")),
                Err(sqlx::error::Error::RowNotFound) => Err(warp::reject::custom(Forbidden)),
                Err(e) => {
                    eprintln!("{:?}", e);
                    Err(warp::reject::custom(InternalError))
                }
            }
        }
    })
}
