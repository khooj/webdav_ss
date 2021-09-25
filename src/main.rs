use clap::{App, Arg};
use hyper::{
    service::{make_service_fn, service_fn},
    Server,
};
use std::{convert::Infallible, net::SocketAddr, str::FromStr};
use webdav_handler::memls::MemLs;
use webdav_handler::DavHandler;
use webdav_handler::{fs::DavFileSystem, localfs::LocalFs, memfs::MemFs};

mod aggregate;
mod configuration;
mod repository;

use aggregate::AggregateBuilder;
use configuration::{Configuration, Filesystem, FilesystemType};
use repository::MemoryRepository;

fn get_backend_by_type(typ: FilesystemType, fs: &Filesystem) -> Box<dyn DavFileSystem> {
    match typ {
        FilesystemType::FS => LocalFs::new(fs.path.as_ref().unwrap(), false, false, false),
        FilesystemType::Mem => MemFs::new(),
    }
}

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

    let matches = App::new("webdav_ss")
        .version("0.1")
        .author("Igor Gilmutdinov <bladoff@gmail.com>")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("sets custom config file")
                .takes_value(true),
        )
        .get_matches();

    let config = matches.value_of("config").unwrap_or("webdav_ss.yml");

    let config = Configuration::new(config).expect("can't get configuration");

    let addr = SocketAddr::from_str(&format!("{}:{}", config.app.host, config.app.port))
        .expect("can't parse host and port");
    let mut fs = AggregateBuilder::new(Box::new(MemoryRepository::new()));

    for fss in config.filesystems {
        fs = fs
            .add_route((&fss.mount_path, get_backend_by_type(fss.typ, &fss)))
            .unwrap();
    }

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
