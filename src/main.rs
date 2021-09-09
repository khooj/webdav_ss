use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use webdav_handler::DavHandler;
use webdav_handler::fakels::FakeLs;
use webdav_handler::localfs::LocalFs;
use webdav_handler::memfs::MemFs;
use webdav_handler::memls::MemLs;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello".into()))
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000);
    let dir = "/tmp";
    let dav_server = DavHandler::builder()
        .filesystem(MemFs::new())
        .locksystem(MemLs::new())
        .build_handler();

    let mave_svc = make_service_fn(move |_conn| {
        let dav_server = dav_server.clone();
        async move {
            let func = move |req| {
                let dav_server = dav_server.clone();
                async move {
                    Ok::<_, Infallible>(dav_server.handle(req).await)
                }
            };
            Ok::<_, Infallible>(service_fn(func))
        }
    });

    let server = Server::bind(&addr).serve(mave_svc);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
