use super::{
    aggregate::AggregateBuilder,
    backend::s3_backend::S3Backend,
    configuration::{Configuration, Filesystem, FilesystemType},
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

async fn get_backend_by_type(typ: FilesystemType, fs: &Filesystem) -> Box<dyn DavFileSystem> {
    match typ {
        FilesystemType::FS => {
            // TODO: move dir check
            let p = fs.path.as_ref().unwrap();
            if let Err(_) = std::fs::metadata(p) {
                std::fs::create_dir_all(p).unwrap();
            }
            LocalFs::new(fs.path.as_ref().unwrap(), false, false, false)
        }
        FilesystemType::Mem => MemFs::new(),
        FilesystemType::S3 => S3Backend::new(
            fs.url.as_ref().unwrap(),
            fs.region.as_ref().unwrap(),
            fs.bucket.as_ref().unwrap(),
        )
        .await
        .unwrap(),
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
                .add_route((&fss.mount_path, get_backend_by_type(fss.typ, &fss).await))
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
