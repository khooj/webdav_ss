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
use std::convert::Infallible;
use tracing::{error, instrument};
use webdav_handler::memls::MemLs;
use webdav_handler::DavHandler;
use webdav_handler::{fs::DavFileSystem, localfs::LocalFs, memfs::MemFs};

macro_rules! cfg_feature {
    (
        #![$meta:meta]
        $($item:item)*
    ) => {
        $(
            #[cfg($meta)]
            $item
        )*
    }
}

cfg_feature!(
    #![all(not(feature = "tls"))]

    use std::{net::SocketAddr, str::FromStr};
);

cfg_feature!(
    #![all(feature = "tls")]

    mod tls;
    use self::tls::build_tls;
);

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
    addr: String,
    dav_server: DavHandler,
    #[cfg(feature = "tls")]
    key: String,
    #[cfg(feature = "tls")]
    cert: String,
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

        #[cfg(feature = "tls")]
        let key = config.app.key;
        #[cfg(feature = "tls")]
        let cert = config.app.cert;

        Application {
            addr,
            dav_server,
            #[cfg(feature = "tls")]
            key,
            #[cfg(feature = "tls")]
            cert,
        }
    }

    #[instrument(skip(self))]
    pub async fn run(self) {
        let dav_server = self.dav_server;
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

        #[cfg(feature = "tls")]
        let srv = Server::builder(
            build_tls(&self.addr, &self.cert, &self.key)
                .await
                .expect("can't build tls connector"),
        )
        .serve(make_svc);

        #[cfg(not(feature = "tls"))]
        let addr = SocketAddr::from_str(&self.addr).expect("can't parse host and port");
        #[cfg(not(feature = "tls"))]
        let srv = Server::bind(&addr).serve(make_svc);

        if let Err(e) = srv.await {
            error!("error running server: {}", e);
        }
    }
}
