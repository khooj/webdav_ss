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
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr};
use tracing::{error, instrument};
use warp::Filter;
use webdav_handler::memls::MemLs;
use webdav_handler::warp::dav_handler;
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
    compression: bool,
}

impl Application {
    pub async fn build(config: Configuration) -> Application {
        let addr = format!("{}:{}", config.app.host, config.app.port);
        let mut fs = AggregateBuilder::new();

        let enc = config.encryption.unwrap_or_default();
        for fss in config.filesystems {
            fs = fs.add_route((
                &fss.mount_path,
                get_encrypted(
                    fss.encryption.unwrap_or(enc.clone()),
                    get_backend_by_type(fss.fs).await,
                ),
            ));
        }

        fs = fs.set_props_storage(get_props_storage_by_conf(
            config
                .prop_storage
                .expect("cant determine prop_storage type"),
        ));

        let dav_server = DavHandler::builder()
            .filesystem(fs.build().expect("cant build aggregate"))
            .locksystem(MemLs::new())
            .build_handler();

        Application {
            addr,
            dav_server,
            compression: config.compression.unwrap_or(false),
        }
    }

    #[instrument(skip(self))]
    pub async fn run(self) {
        let dav_server = self.dav_server;
        let addr = SocketAddr::from_str(&self.addr).expect("can't parse host and port");
        let f = dav_handler(dav_server);

        if self.compression {
            warp::serve(f.with(warp::filters::compression::gzip()))
                .run(addr)
                .await
        } else {
            warp::serve(f).run(addr).await
        }
    }
}
