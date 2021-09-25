use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use hyper::StatusCode;
use rusoto_core::{credential::EnvironmentProvider, Client, HttpClient, Region};
use rusoto_s3::S3Client;
use std::marker::PhantomData;
use std::{collections::HashMap, time::SystemTime};
use tokio::io::BufWriter;
use tracing::instrument;
use webdav_handler::memfs::MemFs;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

#[derive(Clone)]
pub struct S3 {
    name: String,
    memfs: Box<MemFs>,
    client: S3Client,
}

impl S3 {
    pub fn new(name: &str, region: &str, bucket: &str) -> Result<Self> {
        let name = name.to_owned();
        let region = region.parse()?;
        let creds = EnvironmentProvider::default();
        let client = Client::new_with(creds, HttpClient::new()?);
        let client = S3Client::new_with_client(client, region);

        Ok(S3 {
            name,
            client,
            memfs: MemFs::new(),
        })
    }
}

#[derive(Debug)]
struct S3OpenFile {
    is_new: bool,
    path: DavPath,
    options: OpenOptions,
    buf: Vec<u8>,
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
            self.buf.extend_from_slice(buf.chunk());
            Ok(())
        }
        .boxed()
    }

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {
        async move {
            self.buf.extend(buf.iter());
            Ok(())
        }
        .boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {
        async move { Ok(bytes::Bytes::new()) }.boxed()
    }

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<u64> {
        async move { Ok(0u64) }.boxed()
    }

    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        async move { Ok(()) }.boxed()
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

impl DavFileSystem for S3 {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        meta: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        async move { self.memfs.read_dir(path, meta).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move { self.memfs.metadata(path).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move { self.memfs.create_dir(path).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move { self.memfs.remove_file(path).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move { self.memfs.remove_dir(path).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move { self.memfs.rename(from, to).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move { self.memfs.copy(from, to).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        async move { true }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        async move { self.memfs.patch_props(path, patch).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        self.memfs.get_prop(path, prop)
    }

    #[instrument(level = "debug", skip(self))]
    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        self.memfs.get_props(path, do_content)
    }
}
