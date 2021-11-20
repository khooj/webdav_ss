use crate::backend::normalized_path::NormalizedPath;

use super::metadata::S3MetaData;
use bytes::Buf;
use futures_util::FutureExt;
use s3::Bucket;
use std::io::{Cursor, SeekFrom};
use std::time::SystemTime;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{debug, instrument};
use webdav_handler::fs::{DavDirEntry, DavFile, DavMetaData, FsError, FsFuture, OpenOptions};

pub trait S3File: DavFile {
    fn new(
        metadata: S3MetaData,
        buf: Vec<u8>,
        new: bool,
        opts: OpenOptions,
        path: NormalizedPath,
        client: Bucket,
    ) -> Self;
}

pub struct S3DirEntry {
    pub metadata: Box<dyn DavMetaData>,
    pub name: Vec<u8>,
}

impl DavDirEntry for S3DirEntry {
    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(self.metadata.clone()) }.boxed()
    }

    fn name(&self) -> Vec<u8> {
        self.name.clone()
    }
}
