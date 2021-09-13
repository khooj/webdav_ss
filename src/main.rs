use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::ffi::OsString;
use std::str::FromStr;
use webdav_handler::memfs::MemFs;
use webdav_handler::memls::MemLs;
use webdav_handler::DavHandler;

mod aggregate;
mod repository;

use aggregate::AggregateBuilder;
use repository::MemoryRepository;

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello".into()))
}

#[tokio::main]
async fn main() {
    let addr = ([127, 0, 0, 1], 3000).into();
    let fs = AggregateBuilder::new(Box::new(MemoryRepository::new()))
        .add_route(("/fs1", MemFs::new())).unwrap()
        .add_route(("/fs2", MemFs::new())).unwrap();
    let dav_server = DavHandler::builder()
        .filesystem(fs.build())
        .locksystem(MemLs::new())
        .build_handler();

    let make_svc = make_service_fn(move |_conn| {
        let dav_server = dav_server.clone();
        async move {
            let func = move |req| {
                let dav_server = dav_server.clone();
                async move { Ok::<_, Infallible>(dav_server.handle(req).await) }
            };
            Ok::<_, Infallible>(service_fn(func))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
