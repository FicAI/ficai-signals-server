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
    pub message: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ErrorWrap {
    pub error: Error,
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
    let (status, message) = if r.is_not_found() {
        (StatusCode::NOT_FOUND, "not found".to_string())
    } else if let Some(BadRequest(message)) = r.find() {
        (StatusCode::BAD_REQUEST, message.to_string())
    } else if let Some(Forbidden {}) = r.find() {
        (StatusCode::FORBIDDEN, "forbidden".to_string())
    } else if let Some(InternalError {}) = r.find() {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error".to_string(),
        )
    } else if let Some(AccountAlreadyExists {}) = r.find() {
        (StatusCode::CONFLICT, "account already exists".to_string())
    } else if r
        .find::<warp::filters::body::BodyDeserializeError>()
        .is_some()
    {
        eprintln!("body deserialization error: {:#?}", r);
        (StatusCode::BAD_REQUEST, "bad request body".to_string())
    } else if r.find::<warp::reject::InvalidQuery>().is_some() {
        eprintln!("invalid query error: {:#?}", r);
        (StatusCode::BAD_REQUEST, "bad request query".to_string())
    } else if r.find::<warp::reject::MethodNotAllowed>().is_some() {
        eprintln!("method not allowed rejection: {:#?}", r);
        (
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed".to_string(),
        )
    } else {
        eprintln!("uhandled rejection: {:#?}", r);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error".to_string(),
        )
    };

    let json = warp::reply::json(&ErrorWrap {
        error: Error { message },
    });
    Ok(warp::reply::with_status(json, status))
}
