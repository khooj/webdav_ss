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

#[derive(Clone)]
pub struct S3Backend {
    memfs: Box<MemFs>,
    client: Bucket,
}

impl S3Backend {
    pub async fn new(url: &str, region: &str, bucket: &str) -> Result<Box<dyn DavFileSystem>> {
        let url = url.to_owned();
        let region = Region::Custom {
            endpoint: url.clone(),
            region: region.parse()?,
        };
        let creds = Credentials::from_env()?;
        let bucket = bucket.to_owned();
        let config = BucketConfiguration::private();
        let resp = Bucket::create(&bucket, region, creds, config)
            .await
            .expect("cant create bucket");
        if !resp.success() {
            error!(response_code = resp.response_code, response_text = %resp.response_text);
            return Err(anyhow!("cant create bucket"));
        }
        let bucket = resp.bucket;

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
    metadata: S3MetaData,
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

#[derive(derivative::Derivative)]
#[derivative(Debug, Clone, Default)]
struct S3MetaData {
    path: String,
    len: u64,
    #[derivative(Default(value = "SystemTime::now()"))]
    modified: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    accessed: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    created: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    status_changed: SystemTime,
    executable: bool,
}

impl S3MetaData {
    fn extract_unixtime_or_zero(value: &str) -> SystemTime {
        if let Ok(k) = value.parse() {
            std::time::UNIX_EPOCH + Duration::from_secs(k)
        } else {
            SystemTime::now()
        }
    }

    fn extract_from_tags(len: u64, path: String, tags: Tagging) -> Self {
        let mut metadata = S3MetaData::default();
        metadata.len = len;
        metadata.path = path;

        for kv in tags.tag_set.tags.into_iter() {
            let k = kv.key();
            let v = kv.value();
            match &kv.key().as_str() {
                &"modified" => metadata.modified = S3MetaData::extract_unixtime_or_zero(&v),
                &"accessed" => metadata.accessed = S3MetaData::extract_unixtime_or_zero(&v),
                &"created" => metadata.created = S3MetaData::extract_unixtime_or_zero(&v),
                &"status_changed" => {
                    metadata.status_changed = S3MetaData::extract_unixtime_or_zero(&v)
                }
                _ => {}
            }
        }

        metadata
    }

    fn as_unixtime(t: SystemTime) -> String {
        if let Ok(n) = t.duration_since(std::time::UNIX_EPOCH) {
            n.as_secs().to_string()
        } else {
            "0".to_owned()
        }
    }

    fn as_metadata(&self) -> Vec<(String, String)> {
        let modified = S3MetaData::as_unixtime(self.modified);
        let accessed = S3MetaData::as_unixtime(self.accessed);
        let created = S3MetaData::as_unixtime(self.created);
        let status_changed = S3MetaData::as_unixtime(self.status_changed);
        vec![
            ("modified".into(), modified),
            ("accessed".into(), accessed),
            ("created".into(), created),
            ("status_changed".into(), status_changed),
        ]
    }
}

impl DavMetaData for S3MetaData {
    fn len(&self) -> u64 {
        self.len
    }

    fn modified(&self) -> FsResult<SystemTime> {
        Ok(self.modified)
    }

    fn is_dir(&self) -> bool {
        std::path::PathBuf::from(&self.path).is_dir()
    }

    fn accessed(&self) -> FsResult<SystemTime> {
        Ok(self.accessed)
    }

    fn created(&self) -> FsResult<SystemTime> {
        Ok(self.created)
    }

    fn status_changed(&self) -> FsResult<SystemTime> {
        Ok(self.status_changed)
    }

    fn executable(&self) -> FsResult<bool> {
        Ok(self.executable)
    }
}

struct S3DirEntry {}

impl DavDirEntry for S3DirEntry {
    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            Ok(Box::new(S3MetaData::extract_from_tags(
                0,
                "/".to_owned(),
                Tagging {
                    tag_set: TagSet { tags: vec![] },
                },
            )) as Box<dyn DavMetaData>)
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
            let path = S3Backend::normalize_path(path.clone());
            let mut is_new = false;
            let mut buf = vec![];
            let (head, code) = self
                .client
                .head_object(&path)
                .await
                .map_err(|e| FsError::GeneralFailure)?;

            debug!(reason = "open head object", code = code);
            if code != 200 {
                is_new = true;
            } else {
                let (obj, code) = self
                    .client
                    .get_object(&path)
                    .await
                    .map_err(|e| FsError::GeneralFailure)?;

                if code != 200 {
                    error!(reason = "cant get object", code = code);
                    return Err(FsError::GeneralFailure);
                }

                debug!(reason = "received data", length = obj.len());
                buf = obj;
            }

            debug!(is_new = %is_new, path = %path);
            let (mut tags, code) = self.client.get_object_tagging(path.clone()).await.unwrap();

            if code != 200 {
                tags = Some(Tagging {
                    tag_set: TagSet { tags: vec![] },
                });
            }

            debug!(tags = ?tags);
            let len = head.content_length.unwrap_or(0i64) as u64;
            let metadata = S3MetaData::extract_from_tags(
                len,
                path.clone(),
                tags.unwrap_or(Tagging {
                    tag_set: TagSet { tags: vec![] },
                }),
            );

            let cursor = Cursor::new(buf);
            Ok(Box::new(S3OpenFile {
                metadata,
                cursor,
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
            let path = S3Backend::normalize_path(path.clone());
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
            let path = S3Backend::normalize_path(path.clone());
            let (head, code) = self.client.head_object(path.to_string()).await.unwrap();
            if code != 200 {
                debug!(reason = "head object error", code = code);
                return Err(FsError::NotFound);
            }
            let (mut tags, code) = self
                .client
                .get_object_tagging(path.to_string())
                .await
                .unwrap();
            if code != 200 {
                debug!(reason = "tag object error", code = code);
                tags = Some(Tagging {
                    tag_set: TagSet { tags: vec![] },
                });
            }

            let len = head.content_length.unwrap_or(0i64) as u64;
            Ok(
                Box::new(S3MetaData::extract_from_tags(len, path, tags.unwrap()))
                    as Box<dyn DavMetaData>,
            )
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path = path.as_pathbuf();
            let parent = path.parent().unwrap();
            if parent.ends_with("/") && parent.starts_with("/") {
                return Ok(());
            }

            let prefix = parent
                .strip_prefix("/")
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            let resp = self
                .client
                .list(parent.to_str().unwrap().to_owned(), Some("/".to_owned()))
                .await
                .unwrap();

            debug!(parent = ?path, prefix = %prefix, list_length = resp.len(), elems = ?resp);
            if resp.is_empty() {
                return Err(FsError::NotFound);
            }

            for r in resp {
                if r.prefix == prefix && r.contents.is_empty() {
                    return Err(FsError::NotFound);
                }
            }

            let prefix = format!("{}/.dir", prefix);
            let (resp, code) = self
                .client
                .put_object(prefix.clone(), &[])
                .await
                .unwrap();

            debug!(reason = "creating stub dir file", resp = ?resp, code = code, prefix = %prefix);
            if code != 200 {
                return Err(FsError::GeneralFailure);
            }

            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path = S3Backend::normalize_path(path.clone());
            let (resp, code) = self.client.delete_object(path.to_string()).await.unwrap();

            debug!(method = "remove file", code = code);
            if code != 204 {
                return Err(FsError::NotFound);
            }

            Ok(())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path = path.as_pathbuf();
            let prefix = path.to_str().unwrap().to_owned();
            let objects = self.client.list(prefix.clone(), None).await.unwrap();

            debug!(method = "remove_dir", prefix = %prefix, list = ?objects);
            let mut removed = false;
            for obj in objects
                .into_iter()
                .filter(|p| p.prefix == prefix)
                .flat_map(|f| f.contents)
            {
                self.remove_file(&DavPath::new(&obj.key).unwrap()).await?;
                removed = true;
            }

            if !removed {
                return Err(FsError::NotFound);
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
