use ficai_signals_server::fichub::Meta;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use warp::{Filter as _, Reply};

const TEST_URL: &str = "https://forums.spacebattles.com/threads/nemesis-worm-au.747148/";
const TEST_ID: &str = "NtePoQrV";
const TEST_TITLE: &str = "Nemesis";

#[derive(Deserialize, Debug)]
struct GetMetaQ {
    q: String,
}

#[derive(Serialize, Debug)]
struct Err {
    msg: String,
}

async fn get_meta(q: GetMetaQ) -> http::Response<hyper::Body> {
    println!("fake_fichub get_meta({})", q.q);
    if q.q == TEST_URL {
        warp::reply::json(&Meta {
            id: TEST_ID.into(),
            title: TEST_TITLE.into(),
            source: TEST_URL.into(),
        })
        .into_response()
    } else if q.q == "hang-15" {
        tokio::time::sleep(Duration::from_secs(15)).await;
        warp::reply::json(&Meta {
            id: TEST_ID.into(),
            title: TEST_TITLE.into(),
            source: TEST_URL.into(),
        })
        .into_response()
    } else {
        warp::reply::with_status(
            warp::reply::json(&Err {
                msg: "unknown".into(),
            }),
            http::StatusCode::NOT_FOUND,
        )
        .into_response()
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> eyre::Result<()> {
    let get_meta = warp::path!("api" / "v0" / "meta")
        .and(warp::get())
        .and(warp::query::<GetMetaQ>())
        .then(get_meta);

    warp::serve(get_meta)
        .run(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8081,
        ))
        .await;

    Ok(())
}
