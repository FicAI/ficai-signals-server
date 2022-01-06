use warp::Filter;

#[tokio::main]
async fn main() {
    let hello = warp::path::path("hello").and(warp::path::end()).map(|| "Hello!");

    // todo: graceful shutdown
    warp::serve(hello).run(([127, 0, 0, 1], 8080)).await;
}
