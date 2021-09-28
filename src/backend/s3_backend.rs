use anyhow::{anyhow, Result};
use bytes::Buf;
use futures_util::FutureExt;
use hyper::client::{Client, HttpConnector};
use hyper::server::conn::Http;
use hyper::StatusCode;
use rusty_s3::{Bucket as RustyBucket, Credentials as RustyCredentials, S3Action};
use s3::{creds::Credentials, region::Region, Bucket, S3Error};
use std::io::{BufRead, BufReader, Cursor};
use std::{collections::HashMap, time::Duration, time::SystemTime};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, DuplexStream};
use tracing::{error, instrument};
use webdav_handler::memfs::MemFs;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

#[derive(Clone)]
pub struct S3Backend {
    memfs: Box<MemFs>,
    client: Bucket,
}

impl S3Backend {
    pub fn new(url: &str, region: &str, bucket: &str) -> Result<Box<dyn DavFileSystem>> {
        let url = url.to_owned();
        let client = Client::new();
        let region = Region::Custom {
            endpoint: url.clone(),
            region: region.parse()?,
        };
        // let creds = Credentials::from_profile(Some("minio"))?;
        let creds = Credentials::from_env()?;
        let bucket = bucket.to_owned();
        let mut bucket = Bucket::new(&bucket, region, creds)?;
        bucket.set_path_style();

        Ok(Box::new(S3Backend {
            client: bucket,
            memfs: MemFs::new(),
        }) as Box<dyn DavFileSystem>)
    }

    fn normalize_path(path: DavPath) -> String {
        path.as_pathbuf()
            .strip_prefix("/")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
struct S3OpenFile {
    is_new: bool,
    path: String,
    options: OpenOptions,
    cursor: Cursor<Vec<u8>>,
    #[derivative(Debug = "ignore")]
    client: Bucket,
}

impl DavFile for S3OpenFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            Ok(Box::new(S3MetaData {
                path: self.path.clone(),
                len: 0u64,
            }) as Box<dyn DavMetaData>)
        }
        .boxed()
    }

    fn write_buf<'a>(&'a mut self, buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        async move {
            let b = buf.chunk();
            self.cursor.write(b).await.unwrap();
            Ok(())
        }
        .boxed()
    }

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {
        use bytes::Buf;
        async move {
            self.cursor.write(buf.chunk()).await.unwrap();
            Ok(())
        }
        .boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {
        async move {
            let mut b = Vec::with_capacity(count);
            b.resize(count, 0);
            self.cursor.read(b.as_mut()).await.unwrap();
            Ok(bytes::Bytes::from(b))
        }
        .boxed()
    }

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<u64> {
        async move { Ok(self.cursor.seek(pos).await.unwrap()) }.boxed()
    }

    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        use std::io::Read;
        use tokio_util::io::ReaderStream;
        let data = self.cursor.clone();

        async move {
            let (_, code) = self
                .client
                .put_object(self.path.to_string(), data.chunk())
                .await
                .unwrap();

            if code != 200 {
                Err(FsError::GeneralFailure)
            } else {
                Ok(())
            }
        }
        .boxed()
    }
}

#[derive(Debug, Clone)]
struct S3MetaData {
    path: String,
    len: u64,
}

impl S3MetaData {}

impl DavMetaData for S3MetaData {
    fn len(&self) -> u64 {
        self.len
    }

    fn modified(&self) -> FsResult<SystemTime> {
        Ok(SystemTime::now())
    }

    fn is_dir(&self) -> bool {
        std::path::PathBuf::from(&self.path).is_dir()
    }

    fn accessed(&self) -> FsResult<SystemTime> {
        Ok(SystemTime::now())
    }

    fn created(&self) -> FsResult<SystemTime> {
        Ok(SystemTime::now())
    }

    fn status_changed(&self) -> FsResult<SystemTime> {
        Ok(SystemTime::now())
    }

    fn executable(&self) -> FsResult<bool> {
        Ok(false)
    }
}

struct S3DirEntry {}

impl DavDirEntry for S3DirEntry {
    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            Ok(Box::new(S3MetaData {
                len: 0,
                path: "/".to_owned(),
            }) as Box<dyn DavMetaData>)
        }
        .boxed()
    }

    fn name(&self) -> Vec<u8> {
        "asd".into()
    }
}

impl DavFileSystem for S3Backend {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let mut is_new = true;
            let _ = match self.client.head_object(&path.to_string()).await {
                Ok(k) => {
                    is_new = false;
                    k
                }
                Err(e) => {
                    error!(err = %e);
                    return Err(FsError::GeneralFailure);
                }
            };

            let cursor = Cursor::new(vec![]);
            Ok(Box::new(S3OpenFile {
                cursor,
                is_new,
                options,
                path: S3Backend::normalize_path(path.clone()),
                client: self.client.clone(),
            }) as Box<dyn DavFile>)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        meta: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        async move {
            let objects = self.client.list(path.to_string(), None).await.unwrap();

            let s = objects
                .into_iter()
                .map(|e| Box::new(S3DirEntry {}) as Box<dyn DavDirEntry>);
            let s = futures_util::stream::iter(s);
            let s = Box::pin(s) as FsStream<Box<dyn DavDirEntry>>;

            Ok(s)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let head = self.client.head_object(path.to_string()).await.unwrap();
            Ok(Box::new(S3MetaData {
                len: 0,
                path: S3Backend::normalize_path(path.clone()),
            }) as Box<dyn DavMetaData>)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move { Ok(()) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let resp = self.client.delete_object(path.to_string()).await.unwrap();
            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let objects = self.client.list(path.to_string(), None).await.unwrap();

            for obj in objects.into_iter().flat_map(|f| f.contents) {
                self.remove_file(&DavPath::new(&obj.key).unwrap()).await?;
            }
            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            // let resp = self
            //     .client
            //     .copy_object(rusoto_s3::CopyObjectRequest {
            //         bucket: self.bucket.clone(),
            //         copy_source: from.to_string(),
            //         key: to.to_string(),
            //         ..rusoto_s3::CopyObjectRequest::default()
            //     })
            //     .await
            //     .unwrap();

            // let resp = self
            //     .client
            //     .delete_object(rusoto_s3::DeleteObjectRequest {
            //         bucket: self.bucket.clone(),
            //         key: from.to_string(),
            //         ..rusoto_s3::DeleteObjectRequest::default()
            //     })
            //     .await
            //     .unwrap();

            Err(FsError::NotImplemented)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            // let resp = self
            //     .client
            //     .copy_object(rusoto_s3::CopyObjectRequest {
            //         bucket: self.bucket.clone(),
            //         copy_source: from.to_string(),
            //         key: to.to_string(),
            //         ..rusoto_s3::CopyObjectRequest::default()
            //     })
            //     .await
            //     .unwrap();

            Err(FsError::NotImplemented)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        async move { false }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        async move { Err(FsError::NotImplemented) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        async move { Err(FsError::NotImplemented) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        async move { Err(FsError::NotImplemented) }.boxed()
    }
}
