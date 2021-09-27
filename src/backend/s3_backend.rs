use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use hyper::StatusCode;
use rusoto_core::{credential::EnvironmentProvider, Client, HttpClient, Region};
use rusoto_s3::{Bucket, S3Client, S3};
use std::io::{BufRead, BufReader, Cursor};
use std::marker::PhantomData;
use std::ops::Deref;
use std::{collections::HashMap, time::SystemTime};
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

struct S3ClientWrapper<'a>(&'a S3Client);

impl<'a> std::fmt::Debug for S3ClientWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s3 client wrapper")
    }
}

impl<'a> Deref for S3ClientWrapper<'a> {
    type Target = S3Client;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[derive(Clone)]
pub struct S3Backend {
    name: String,
    bucket: String,
    memfs: Box<MemFs>,
    client: S3Client,
}

impl S3Backend {
    pub fn new(name: &str, region: &str, bucket: &str) -> Result<Self> {
        let name = name.to_owned();
        let region = region.parse()?;
        let creds = EnvironmentProvider::default();
        let client = Client::new_with(creds, HttpClient::new()?);
        let client = S3Client::new_with_client(client, region);

        Ok(S3Backend {
            name,
            bucket: bucket.to_owned(),
            client,
            memfs: MemFs::new(),
        })
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
struct S3OpenFile {
    is_new: bool,
    path: DavPath,
    options: OpenOptions,
    bucket: String,
    cursor: Cursor<Vec<u8>>,
    #[derivative(Debug = "ignore")]
    client: S3Client,
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
        use tokio_util::io::ReaderStream;
        let data = self.cursor.clone();

        async move {
            let _ = self
                .client
                .put_object(rusoto_s3::PutObjectRequest {
                    bucket: self.bucket.clone(),
                    key: self.path.to_string(),
                    body: Some(rusoto_core::ByteStream::new(ReaderStream::new(data))),
                    ..rusoto_s3::PutObjectRequest::default()
                })
                .await
                .unwrap();
            Ok(())
        }
        .boxed()
    }
}

#[derive(Debug, Clone)]
struct S3MetaData {
    path: DavPath,
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
        self.path.is_collection()
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
                path: DavPath::new("/").unwrap(),
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
            let _ = match self
                .client
                .head_object(rusoto_s3::HeadObjectRequest {
                    bucket: self.bucket.clone(),
                    key: path.to_string(),
                    ..rusoto_s3::HeadObjectRequest::default()
                })
                .await
            {
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
                bucket: self.bucket.clone(),
                is_new,
                options,
                path: path.clone(),
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
            let objects = self
                .client
                .list_objects_v2(rusoto_s3::ListObjectsV2Request {
                    bucket: self.bucket.clone(),
                    prefix: Some(path.to_string()),
                    ..rusoto_s3::ListObjectsV2Request::default()
                })
                .await
                .unwrap();

            let s = objects
                .contents
                .unwrap_or(vec![])
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
            let head = self
                .client
                .head_object(rusoto_s3::HeadObjectRequest {
                    bucket: self.bucket.clone(),
                    key: path.to_string(),
                    ..rusoto_s3::HeadObjectRequest::default()
                })
                .await
                .unwrap();
            Ok(Box::new(S3MetaData {
                len: 0,
                path: path.clone(),
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
            let resp = self
                .client
                .delete_object(rusoto_s3::DeleteObjectRequest {
                    bucket: self.bucket.clone(),
                    key: path.to_string(),
                    ..rusoto_s3::DeleteObjectRequest::default()
                })
                .await
                .unwrap();
            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let objects = self
                .client
                .list_objects_v2(rusoto_s3::ListObjectsV2Request {
                    bucket: self.bucket.clone(),
                    prefix: Some(path.to_string()),
                    ..rusoto_s3::ListObjectsV2Request::default()
                })
                .await
                .unwrap();

            for obj in objects.contents.unwrap_or(vec![]) {
                self.remove_file(&DavPath::new(&obj.key.unwrap()).unwrap())
                    .await?;
            }
            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let resp = self
                .client
                .copy_object(rusoto_s3::CopyObjectRequest {
                    bucket: self.bucket.clone(),
                    copy_source: from.to_string(),
                    key: to.to_string(),
                    ..rusoto_s3::CopyObjectRequest::default()
                })
                .await
                .unwrap();

            let resp = self
                .client
                .delete_object(rusoto_s3::DeleteObjectRequest {
                    bucket: self.bucket.clone(),
                    key: from.to_string(),
                    ..rusoto_s3::DeleteObjectRequest::default()
                })
                .await
                .unwrap();

            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let resp = self
                .client
                .copy_object(rusoto_s3::CopyObjectRequest {
                    bucket: self.bucket.clone(),
                    copy_source: from.to_string(),
                    key: to.to_string(),
                    ..rusoto_s3::CopyObjectRequest::default()
                })
                .await
                .unwrap();

            Ok(())
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
