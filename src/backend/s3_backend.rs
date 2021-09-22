use anyhow::{anyhow, Result};
use hyper::StatusCode;
use s3::creds::Credentials;
use s3::region::Region;
use s3::{bucket::Bucket, serde_types::HeadObjectResult};
use std::{collections::HashMap, time::SystemTime};
use tracing::instrument;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

pub struct S3 {
    name: String,
    region: Region,
    credentials: Credentials,
    bucket: Bucket,
}

impl S3 {
    pub fn new(name: &str, region: &str, bucket: &str) -> Result<Self> {
        let name = name.to_owned();
        let region = region.parse()?;
        let creds = Credentials::from_profile(Some("minio"))?;
        let bucket = Bucket::new(bucket, region, creds)?;

        Ok(S3 {
            name,
            region,
            credentials: creds,
            bucket,
        })
    }
}

struct S3OpenFile {
    is_new: bool,
    bucket: &Bucket,
    path: DavPath,
    options: OpenOptions,
}

impl DavFile for S3OpenFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            Ok(S3MetaData {
                bucket: self.bucket,
                path: self.path.clone(),
                len: 0u64,
            })
        }
        .boxed()
    }

    fn write_buf<'a>(&'a mut self, buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {}

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {}

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {}

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<u64> {}

    fn flush<'a>(&'a mut self) -> FsFuture<()> {}
}

struct S3MetaData {
    bucket: &Bucket,
    path: DavPath,
    len: u64,
}

impl S3MetaData {
    async fn get_header(&self) -> Result<HeadObjectResult> {
        let (head, code) = self.bucket.head_object(&self.path.as_url_string()).await?;
        if code != 200 {
            return Err(anyhow!("error"));
        }

        Ok(head)
    }

    async fn get_metadata(&self) -> Result<HashMap<String, String>> {
        let head = self.get_header().await?;
        head.metadata.ok_or(HashMap::new())
    }
}

impl DavMetaData for S3MetaData {
    fn len(&self) -> u64 {
        self.len
    }

    fn modified(&self) -> FsResult<SystemTime> {
        async move {
            let header = self.get_header().await?;
            Ok(header
                .last_modified
                .map_or(SystemTime::now(), |s| s.parse()?))
        }
        .boxed()
    }

    fn is_dir(&self) -> bool {
        self.path.is_collection()
    }

    fn accessed(&self) -> FsResult<SystemTime> {}

    fn created(&self) -> FsResult<SystemTime> {}

    fn status_changed(&self) -> FsResult<SystemTime> {}

    fn executable(&self) -> FsResult<bool> {}
}

impl DavFileSystem for S3 {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let (head_result, code) = self.bucket.head_object(&path.as_url_string()).await?;
            let mut is_new = false;
            let mut created = SystemTime::now();
            match code {
                404 => {
                    is_new = true;
                }
                403 => return Err(anyhow!("forbidden")),
                200 => {
                    created = head_result
                        .metadata
                        .as_ref()
                        .unwrap_or(&HashMap::new())
                        .get("created")
                        .map_or(created, |v| v.parse()?);
                }
                _ => {
                    panic!("not possible code")
                }
            }

            S3OpenFile {
                is_new,
                bucket: &self.bucket,
                path: path.clone(),
                options,
            }
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
            let (route, path) = self.find_route(&path)?;
            Ok(route.read_dir(&path, meta).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.metadata(&path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.create_dir(&path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.remove_file(&path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.remove_dir(&path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route.rename(&from, &to).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route.copy(&from, &to).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        future::ready(true).boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.patch_props(&path, patch).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.get_prop(&path, prop).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.get_props(&path, do_content).await?)
        }
        .boxed()
    }
}

fn check_bucket() -> Result<()> {
    let creds = Credentials::from_profile(Some("minio"))?;
    let bucket = Bucket::new("test", "eu-central-1".parse()?, creds)?;
}
