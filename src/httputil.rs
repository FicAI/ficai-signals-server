use http::{Response, StatusCode};
use hyper::Body;

pub fn bad_request(message: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(message.into())
        .unwrap()
}

pub fn internal_error(message: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(message.into())
        .unwrap()
}
