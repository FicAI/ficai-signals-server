use argon2::{Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _};
use base64ct::Encoding as _;
use eyre::{eyre, WrapErr};
use http::header::SET_COOKIE;
use http::{Response, StatusCode};
use hyper::Body;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tap::prelude::*;
use warp::{
    reply::{json, with_header, with_status},
    Filter, Rejection, Reply,
};

use crate::httputil::{AccountAlreadyExists, BadRequest, Empty, Forbidden, InternalError};
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

#[derive(Serialize, Debug, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountSession {
    pub id: i64,
    email: String,
    #[serde(skip_serializing)]
    session_id: Vec<u8>,
}

impl AccountSession {
    async fn create(id: i64, email: String, db: &DB) -> eyre::Result<Self> {
        let mut session_id = [0u8; SESSION_ID_BYTES];
        for _ in 0..3 {
            OsRng.fill_bytes(&mut session_id);
            let insert_result = sqlx::query("insert into session (id, account_id) values ($1, $2)")
                .bind(&session_id[..])
                .bind(id)
                .execute(db)
                .await;
            match insert_result {
                Ok(_) => {
                    return Ok(Self {
                        id,
                        email,
                        session_id: session_id.to_vec(),
                    })
                }
                Err(sqlx::Error::Database(db_err))
                    if db_err.code() == Some(CONSTRAINT_VIOLATION_SQLSTATE.into()) =>
                {
                    continue
                }
                Err(e) => return Err(e).wrap_err("failed to insert new session"),
            }
        }
        Err(eyre!("failed to generate a new session id in 3 attempts"))
    }

    fn cookie_value(&self) -> String {
        base64ct::Base64Unpadded::encode_string(&self.session_id)
    }

    fn to_cookie<'a>(&self, domain: &'a str) -> cookie::Cookie<'a> {
        cookie::Cookie::build(SESSION_COOKIE_NAME, self.cookie_value())
            .domain(domain)
            .path("/")
            .secure(true)
            .http_only(true)
            .permanent()
            .finish()
    }

    fn to_cookie_removal<'a>(&self, domain: &'a str) -> cookie::Cookie<'a> {
        self.to_cookie(domain).tap_mut(|c| c.make_removal())
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountQ {
    email: String,
    password: String,
    beta_key: String,
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
    let row = sqlx::query_scalar::<_, i64>(
        "insert into account (email, password_hash) values ($1, $2) returning id",
    )
    .bind(&q.email)
    .bind(hash)
    .fetch_one(&pool)
    .await;
    let uid = match row {
        Ok(uid) => uid,
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

    let session = AccountSession::create(uid, q.email, &pool)
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            warp::reject::custom(InternalError)
        })?;
    let session_id_cookie = session.to_cookie(domain).to_string();
    Ok(json(&session)
        .pipe(|r| with_status(r, StatusCode::CREATED))
        .pipe(|r| with_header(r, SET_COOKIE, session_id_cookie))
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
    let row = sqlx::query_as::<_, (i64, String)>(
        "select id, password_hash from account where email = $1",
    )
    .bind(&q.email)
    .fetch_optional(&db)
    .await
    .map_err(|e| {
        eprintln!("{:?}", e);
        warp::reject::custom(InternalError)
    })?;
    let (uid, db_hash_string) = match row {
        Some(row) => row,
        None => return Err(warp::reject::custom(Forbidden)),
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
    let session = AccountSession::create(uid, q.email, &db)
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            warp::reject::custom(InternalError)
        })?;
    let session_id_cookie = session.to_cookie(domain).to_string();
    Ok(json(&session)
        .pipe(|r| with_header(r, SET_COOKIE, session_id_cookie))
        .into_response())
}

pub async fn get_session_account(account: AccountSession) -> Result<Response<Body>, Rejection> {
    Ok(json(&account).into_response())
}

pub async fn delete_session(
    session: AccountSession,
    pool: DB,
    domain: &str,
) -> Result<Response<Body>, Rejection> {
    let rows_affected = sqlx::query("delete from session where id = ")
        .bind(&session.session_id)
        .execute(&pool)
        .await
        .map_err(|e| {
            eprintln!("error deleting session: {:#?}", e);
            warp::reject::custom(InternalError)
        })?
        .rows_affected();
    if 1 == rows_affected {
        Ok(json(&Empty {})
            .pipe(|r| with_header(r, SET_COOKIE, session.to_cookie_removal(domain).to_string()))
            .into_response())
    } else {
        // This may mean the account was deleted in between validating their session and getting to
        // this point, which means the current request is racing against a delete.
        Err(warp::reject::custom(InternalError))
    }
}

pub fn optional_authenticate(
    db: DB,
) -> impl Filter<Extract = (Option<AccountSession>,), Error = Rejection> + Clone {
    warp::cookie::optional(SESSION_COOKIE_NAME).and_then(move |cookie: Option<String>| {
        let db = db.clone();
        async move {
            let cookie = match cookie {
                Some(cookie) => cookie,
                None => return Ok(None),
            };
            let cookie = base64ct::Base64Unpadded::decode_vec(&cookie)
                .map_err(|_| warp::reject::custom(BadRequest("invalid auth cookie".into())))?;

            let row = sqlx::query_as::<_, AccountSession>(
                r#"
                select a.id, a.email
                    , s.id as session_id
                from session s
                join account a
                    on a.id = s.account_id
                where s.id = $1"#,
            )
            .bind(&cookie)
            .fetch_optional(&db)
            .await;
            match row {
                Ok(account_session) => Ok(account_session),
                Err(e) => {
                    eprintln!("{:?}", e);
                    Err(warp::reject::custom(InternalError))
                }
            }
        }
    })
}

pub fn authenticate(db: DB) -> impl Filter<Extract = (AccountSession,), Error = Rejection> + Clone {
    optional_authenticate(db).and_then(|account_session: Option<AccountSession>| async {
        account_session.ok_or_else(|| warp::reject::custom(Forbidden))
    })
}
