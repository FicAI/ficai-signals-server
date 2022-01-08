use base64ct::Encoding as _;
use cookie::Cookie;
use http::{Response, StatusCode};
use http::header::SET_COOKIE;
use hyper::Body;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sqlx::Row as _;

use crate::DB;
use crate::httputil::{bad_request, internal_error};

pub const SESSION_COOKIE_NAME: &str = "FicAiSession";

const CONSTRAINT_VIOLATION_SQLSTATE: &str = "23505";

// https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-length
const SESSION_ID_BYTES: usize = 16;


#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserQ {
    email: String,
    password: String,
}

pub async fn create_user<'ctx>(q: CreateUserQ, pool: DB, pepper: impl AsRef<Vec<u8>>, domain: impl AsRef<String>) -> Response<Body> {
    let hash = {
        use argon2::{Argon2, Algorithm::Argon2id, Version::V0x13, Params, PasswordHasher};
        use argon2::password_hash::SaltString;

        let kdf = {
            // https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id
            let params = Params::new(37 * 1024, 1, 1, Some(32)).unwrap();
            Argon2::new_with_secret(pepper.as_ref().as_slice(), Argon2id, V0x13, params).unwrap()
        };
        let salt = SaltString::generate(OsRng);
        match kdf.hash_password(q.password.as_bytes(), &salt) {
            Ok(hash) => hash.to_string(),
            Err(e) =>
                // todo: log e
                return bad_request("invalid password"),
        }
    };
    let row = sqlx::query(r#"insert into "user" (email, password_hash) values ($1, $2) returning id"#)
        .bind(q.email)
        .bind(hash)
        .fetch_one(&pool)
        .await;
    let uid: i64 = match row {
        Ok(row) => row.get("id"),
        Err(sqlx::Error::Database(db_err)) if db_err.code() == Some(CONSTRAINT_VIOLATION_SQLSTATE.into()) =>
            return Response::builder()
                .status(StatusCode::CONFLICT)
                .body("account already exists".into())
                .unwrap(),
        Err(e) =>
            // todo: log e
            return internal_error(Body::empty()),
    };

    let mut session_id = [0u8; SESSION_ID_BYTES];
    let mut inserted = false;
    for _ in 0..3 {
        OsRng.fill_bytes(&mut session_id);
        let insert_result = sqlx::query(r#"insert into session (id, user_id) values ($1, $2)"#)
            .bind(&session_id[..])
            .bind(uid)
            .execute(&pool)
            .await;
        match insert_result {
            Ok(_) => {
                inserted = true;
                break;
            },
            Err(sqlx::Error::Database(db_err)) if db_err.code() == Some(CONSTRAINT_VIOLATION_SQLSTATE.into()) =>
                continue,
            Err(e) =>
                // todo: log e
                return internal_error(Body::empty()),
        }
    }
    if !inserted {
        return internal_error(Body::empty());
    }
    let session_id_string = base64ct::Base64Unpadded::encode_string(&session_id);
    let session_id_cookie = Cookie::build(SESSION_COOKIE_NAME, session_id_string)
        .domain(domain.as_ref())
        .path("/")
        .secure(true)
        .http_only(true)
        .max_age(cookie::time::Duration::days(365 * 10))
        .finish()
        .to_string();

    Response::builder()
        .status(StatusCode::CREATED)
        .header(SET_COOKIE, session_id_cookie)
        .body(Body::empty())
        .unwrap()
}
