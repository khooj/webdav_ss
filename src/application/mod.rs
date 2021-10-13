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
        Filesystem::S3 {
            url,
            region,
            bucket,
            ..
        } => S3Backend::new(&url, &region, &bucket).await.unwrap(),
    }
}

pub struct Application {
    addr: SocketAddr,
    dav_server: DavHandler,
}

impl Application {
    pub async fn build(config: Configuration) -> Application {
        let addr = SocketAddr::from_str(&format!("{}:{}", config.app.host, config.app.port))
            .expect("can't parse host and port");
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

        Application { addr, dav_server }
    }

    #[instrument(skip(self))]
    pub async fn run(self) {
        let dav_server = self.dav_server;
        let addr = self.addr;
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
        if let Err(e) = Server::bind(&addr).serve(make_svc).await {
            error!("error running server: {}", e);
        }
    }
}
