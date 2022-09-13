use crate::{
    backend::{
        encryption::EncryptionWrapper,
        prop_storages::{mem::Memory, yaml::Yaml, PropStorage},
    },
    configuration::{Encryption, PropsStorage},
};

use super::{
    aggregate::AggregateBuilder,
    backend::s3_backend::S3Backend,
    configuration::{Configuration, Filesystem},
};
use hyper::{
    service::{make_service_fn, service_fn},
    Server,
};
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr};
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

fn get_props_storage_by_conf(p: PropsStorage) -> Box<dyn PropStorage> {
    match p {
        PropsStorage::Yaml { path } => Yaml::new(PathBuf::from_str(&path).unwrap()),
        PropsStorage::Mem => Memory::new(),
    }
}

fn get_encrypted(enc: Encryption, fs: Box<dyn DavFileSystem>) -> Box<dyn DavFileSystem> {
    if enc.enable {
        Box::new(EncryptionWrapper::new(&enc.phrase.unwrap(), &enc.nonce.unwrap(), fs).unwrap())
            as Box<dyn DavFileSystem>
    } else {
        fs
    }
}

pub struct Application {
    addr: String,
    dav_server: DavHandler,
}

impl Application {
    pub async fn build(config: Configuration) -> Application {
        let addr = format!("{}:{}", config.app.host, config.app.port);
        let mut fs = AggregateBuilder::new();

        let enc = config.encryption.unwrap_or_default();
        for fss in config.filesystems {
            fs = fs.add_route((
                &fss.mount_path,
                get_encrypted(fss.encryption.unwrap_or(enc.clone()), get_backend_by_type(fss.fs).await),
            ));
        }

        fs = fs.set_props_storage(get_props_storage_by_conf(
            config.prop_storage.unwrap_or(PropsStorage::Mem),
        ));

        let dav_server = DavHandler::builder()
            .filesystem(fs.build().expect("cant build aggregate"))
            .locksystem(MemLs::new())
            .build_handler();

        Application { addr, dav_server }
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
        let addr = SocketAddr::from_str(&self.addr).expect("can't parse host and port");
        let srv = Server::bind(&addr).serve(make_svc);
        if let Err(e) = srv.await {
            error!("error running server: {}", e);
        }
    }
}
