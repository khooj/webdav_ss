mod tls;

use super::{
    aggregate::AggregateBuilder,
    backend::s3_backend::S3Backend,
    configuration::{Configuration, Filesystem},
    repository::MemoryRepository,
};
use hyper::{
    service::{make_service_fn, service_fn},
    Server,
};
use std::{convert::Infallible, net::SocketAddr, str::FromStr};
use tls::build_tls;
use tracing::{error, instrument};
use webdav_handler::memls::MemLs;
use webdav_handler::DavHandler;
use webdav_handler::{fs::DavFileSystem, localfs::LocalFs, memfs::MemFs};

async fn get_backend_by_type(fs: Filesystem) -> Box<dyn DavFileSystem> {
    match fs {
        Filesystem::FS { path } => {
            // TODO: move dir check
            if let Err(_) = std::fs::metadata(&path) {
                std::fs::create_dir_all(&path).unwrap();
            }
            LocalFs::new(&path, false, false, false)
        }
        Filesystem::Mem => MemFs::new(),
        a @ Filesystem::S3 { .. } => S3Backend::new(a).await.unwrap(),
    }
}

struct KeyCert {
    key: String,
    cert: String,
}

pub struct Application {
    addr: String,
    dav_server: DavHandler,
    tls: Option<KeyCert>,
}

impl Application {
    pub async fn build(config: Configuration) -> Application {
        let addr = format!("{}:{}", config.app.host, config.app.port);
        let mut fs = AggregateBuilder::new(Box::new(MemoryRepository::new()));

        for fss in config.filesystems {
            fs = fs
                .add_route((&fss.mount_path, get_backend_by_type(fss.fs).await))
                .unwrap();
        }

        let dav_server = DavHandler::builder()
            .filesystem(fs.build())
            .locksystem(MemLs::new())
            .build_handler();

        let mut tls = None;
        if config.app.tls {
            tls = Some(KeyCert {
                key: config.app.key.unwrap(),
                cert: config.app.cert.unwrap(),
            });
        }

        Application {
            addr,
            dav_server,
            tls,
        }
    }

    #[instrument(skip(self))]
    pub async fn run(self) {
        let dav_server = self.dav_server;

        // rust inherit different signatures for make_svc so for simple solution we just copy-paste it for now.
        if self.tls.is_some() {
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
            let tls = self.tls.unwrap();
            let srv = Server::builder(
                build_tls(&self.addr, &tls.cert, &tls.key)
                    .await
                    .expect("can't build tls connector"),
            )
            .serve(make_svc);

            if let Err(e) = srv.await {
                error!("error running server: {}", e);
            }
        } else {
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
            let addr = SocketAddr::from_str(&self.addr).expect("can't parse host and port");
            let srv = Server::bind(&addr).serve(make_svc);
            if let Err(e) = srv.await {
                error!("error running server: {}", e);
            }
        }
    }
}
