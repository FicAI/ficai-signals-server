use std::borrow::Cow;
use std::convert::Infallible;

use http::StatusCode;
use serde::Serialize;
use warp::reject::Reject;
use warp::{Rejection, Reply};

#[derive(Serialize, Debug)]
pub struct Empty {}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub struct BadRequest(pub Cow<'static, str>);
impl Reject for BadRequest {}

#[derive(Debug)]
pub struct Forbidden;
impl Reject for Forbidden {}

#[derive(Debug)]
pub struct InternalError;
impl Reject for InternalError {}

#[derive(Debug)]
pub struct AccountAlreadyExists;
impl Reject for AccountAlreadyExists {}

pub async fn recover_custom(r: Rejection) -> Result<impl Reply, Infallible> {
    let (status, err) = if r.is_not_found() {
        (
            StatusCode::NOT_FOUND,
            Error {
                code: "not_found".to_string(),
                message: "not found".to_string(),
            },
        )
    } else if let Some(BadRequest(message)) = r.find() {
        (
            StatusCode::BAD_REQUEST,
            Error {
                code: "bad_request".to_string(),
                message: message.to_string(),
            },
        )
    } else if let Some(Forbidden {}) = r.find() {
        (
            StatusCode::FORBIDDEN,
            Error {
                code: "forbidden".to_string(),
                message: "forbidden".to_string(),
            },
        )
    } else if let Some(InternalError {}) = r.find() {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Error {
                code: "internal_server_error".to_string(),
                message: "internal server error".to_string(),
            },
        )
    } else if let Some(AccountAlreadyExists {}) = r.find() {
        (
            StatusCode::CONFLICT,
            Error {
                code: "conflict".to_string(),
                message: "account already exists".to_string(),
            },
        )
    } else if r
        .find::<warp::filters::body::BodyDeserializeError>()
        .is_some()
    {
        eprintln!("body deserialization error: {:#?}", r);
        (
            StatusCode::BAD_REQUEST,
            Error {
                code: "bad_request".to_string(),
                message: "bad request body".to_string(),
            },
        )
    } else if r.find::<warp::reject::InvalidQuery>().is_some() {
        eprintln!("invalid query error: {:#?}", r);
        (
            StatusCode::BAD_REQUEST,
            Error {
                code: "bad_request".to_string(),
                message: "bad request query".to_string(),
            },
        )
    } else if r.find::<warp::reject::MethodNotAllowed>().is_some() {
        eprintln!("method not allowed rejection: {:#?}", r);
        (
            StatusCode::METHOD_NOT_ALLOWED,
            Error {
                code: "method_not_allowed".to_string(),
                message: "method not allowed".to_string(),
            },
        )
    } else {
        eprintln!("uhandled rejection: {:#?}", r);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Error {
                code: "internal_server_error".to_string(),
                message: "internal server error".to_string(),
            },
        )
    };

    let json = warp::reply::json(&err);
    Ok(warp::reply::with_status(json, status))
}
