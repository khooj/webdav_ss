use hyper::{
    service::{make_service_fn, service_fn},
    Server,
};
use std::convert::Infallible;
use webdav_handler::memfs::MemFs;
use webdav_handler::memls::MemLs;
use webdav_handler::DavHandler;

mod aggregate;
mod repository;

use aggregate::AggregateBuilder;
use repository::MemoryRepository;

fn setup_tracing() {
    use tracing_subscriber::{fmt, prelude::*, registry::Registry, EnvFilter};

    let fmt_subscriber = fmt::layer();

    let env_subscriber = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let collector = Registry::default()
        .with(fmt_subscriber)
        .with(env_subscriber);

    tracing_log::LogTracer::init().expect("can't set log tracer");
    tracing::subscriber::set_global_default(collector).expect("can't set global default");
}

#[tokio::main]
async fn main() {
    setup_tracing();

    let addr = ([127, 0, 0, 1], 3000).into();
    let fs = AggregateBuilder::new(Box::new(MemoryRepository::new()))
        .add_route(("/fs1", MemFs::new()))
        .unwrap()
        .add_route(("/", MemFs::new()))
        .unwrap();
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
