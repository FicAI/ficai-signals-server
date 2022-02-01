use std::borrow::Cow;

use http::{Response, StatusCode};
use hyper::Body;
use warp::reject::Reject;
use warp::Rejection;

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

pub async fn recover_custom(r: Rejection) -> Result<Response<Body>, Rejection> {
    if let Some(BadRequest(message)) = r.find() {
        Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(message.clone().into())
            .unwrap())
    } else if let Some(Forbidden {}) = r.find() {
        Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap())
    } else if let Some(InternalError {}) = r.find() {
        Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap())
    } else if let Some(AccountAlreadyExists {}) = r.find() {
        Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .body("account already exists".into())
            .unwrap())
    } else {
        Err(r)
    }
}
