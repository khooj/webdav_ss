use anyhow::{anyhow, Result};
use bytes::Buf;
use futures_util::FutureExt;
use hyper::client::{Client, HttpConnector};
use hyper::server::conn::Http;
use hyper::StatusCode;
use rusty_s3::{Bucket as RustyBucket, Credentials as RustyCredentials, S3Action};
use s3::BucketConfiguration;
use s3::{
    creds::Credentials,
    region::Region,
    serde_types::{TagSet, Tagging},
    Bucket, S3Error,
};
use std::io::{BufRead, BufReader, Cursor, SeekFrom};
use std::path::Path;
use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, DuplexStream};
use tracing::{debug, error, instrument};
use webdav_handler::memfs::MemFs;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};
use super::metadata::S3MetaData;

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct S3OpenFile {
    pub is_new: bool,
    pub path: String,
    pub options: OpenOptions,
    pub cursor: Cursor<Vec<u8>>,
    #[derivative(Debug = "ignore")]
    pub client: Bucket,
    pub metadata: S3MetaData,
}

impl DavFile for S3OpenFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(Box::new(self.metadata.clone()) as Box<dyn DavMetaData>) }.boxed()
    }

    fn write_buf<'a>(&'a mut self, buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        async move {
            let b = buf.chunk();
            self.cursor.write(b).await.unwrap();
            self.metadata.modified = SystemTime::now();
            self.metadata.len += b.len() as u64;
            Ok(())
        }
        .boxed()
    }

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {
        async move {
            self.cursor.write(buf.chunk()).await.unwrap();
            self.metadata.modified = SystemTime::now();
            self.metadata.len += buf.len() as u64;
            Ok(())
        }
        .boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {
        async move {
            let mut b = Vec::with_capacity(count);
            b.resize(count, 0);
            self.cursor.read(b.as_mut()).await.unwrap();
            self.metadata.accessed = SystemTime::now();
            Ok(bytes::Bytes::from(b))
        }
        .boxed()
    }

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<u64> {
        async move { Ok(self.cursor.seek(pos).await.unwrap()) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        let mut data = self.cursor.clone();
        debug!(path = %self.path, length = self.metadata.len);

        async move {
            data.seek(SeekFrom::Start(0)).await.unwrap();
            let (_, code) = self
                .client
                .put_object(self.path.to_string(), data.chunk())
                .await
                .unwrap();

            if code != 200 {
                debug!(reason = "put object unsuccessful", code = code);
                return Err(FsError::GeneralFailure);
            }

            let tags = self.metadata.as_metadata();

            let (_, code) = self
                .client
                .put_object_tagging(&self.path.to_string(), &tags[..])
                .await
                .unwrap();
            if code != 200 {
                debug!(reason = "tag object unsuccessful", code = code);
                return Err(FsError::GeneralFailure);
            }
            Ok(())
        }
        .boxed()
    }
}

pub struct S3DirEntry {}

impl DavDirEntry for S3DirEntry {
    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            Ok(Box::new(S3MetaData::extract_from_tags(
                0,
                "/".to_owned(),
                Tagging {
                    tag_set: TagSet { tags: vec![] },
                },
                false,
            )) as Box<dyn DavMetaData>)
        }
        .boxed()
    }

    fn name(&self) -> Vec<u8> {
        "asd".into()
    }
}