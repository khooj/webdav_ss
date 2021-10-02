use super::{
    entries::{S3DirEntry, S3OpenFile},
    metadata::S3MetaData,
};
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
use std::path::{Path, PathBuf};
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
        if !resp.success() && resp.response_code != 409 {
            error!(response_code = resp.response_code, response_text = %resp.response_text);
            return Err(anyhow!("cant create bucket"));
        }
        let bucket = resp.bucket;

        Ok(Box::new(S3Backend {
            client: bucket,
            memfs: MemFs::new(),
        }) as Box<dyn DavFileSystem>)
    }

    fn normalize_path<T: AsRef<Path>>(path: T) -> String {
        path.as_ref()
            .strip_prefix("/")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    async fn metadata_info(
        &self,
        path: PathBuf,
        is_col: bool,
    ) -> Result<Box<dyn DavMetaData>, FsError> {
        let mut tags = Some(Tagging {
            tag_set: TagSet { tags: vec![] },
        });

        // root dir always exist
        if path.starts_with("/") && path.ends_with("/") {
            return Ok(Box::new(S3MetaData::extract_from_tags(
                0,
                "".into(),
                tags.unwrap(),
                true,
            )) as Box<dyn DavMetaData>);
        }

        let normalized = S3Backend::normalize_path(path.clone());
        let prefix = S3Backend::normalize_path(if is_col { path.join(".dir") } else { path });

        debug!(reason = "trying to head object", col = is_col, prefix = %prefix);
        let (head, code) = self.client.head_object(prefix.clone()).await.unwrap();
        if code != 200 {
            debug!(reason = "head object error", code = code);
            return Err(FsError::NotFound);
        }

        if !is_col {
            let (t, code) = self.client.get_object_tagging(prefix).await.unwrap();
            tags = t;

            if code != 200 {
                debug!(reason = "tag object error", code = code);
                tags = Some(Tagging {
                    tag_set: TagSet { tags: vec![] },
                });
            }
        }

        let len = head.content_length.unwrap_or(0i64) as u64;
        Ok(Box::new(S3MetaData::extract_from_tags(
            len,
            normalized,
            tags.unwrap(),
            is_col,
        )) as Box<dyn DavMetaData>)
    }
}

impl DavFileSystem for S3Backend {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let path = S3Backend::normalize_path(path.as_pathbuf());
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
                false,
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
            let path = S3Backend::normalize_path(path.as_pathbuf());
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
            Ok(self
                .metadata_info(path.as_pathbuf(), path.is_collection())
                .await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let meta = self.metadata_info(path.as_pathbuf(), path.is_collection()).await;
            if let Ok(m) = meta {
                if m.is_dir() {
                    debug!(msg = "dir already exist", path = ?path);
                    return Ok(());
                }
            

            let path = path.as_pathbuf();
            let prefix_dir = format!("{}/.dir", path.to_str().unwrap());
            let parent = path.parent().unwrap();
            if parent.ends_with("/") && parent.starts_with("/") {
                let (resp, code) = self
                    .client
                    .put_object(prefix_dir.clone(), &[])
                    .await
                    .unwrap();

                debug!(reason = "creating stub dir file", resp = ?resp, code = code, prefix = %prefix_dir);
                if code != 200 {
                    return Err(FsError::GeneralFailure);
                }
                return Ok(());
            }

            let meta = self.metadata_info(parent.to_path_buf(), true).await;
            if let Err(e) = meta {
                debug!(msg = "parent folder does not exist", parent = ?parent, err = %e);
                return Err(FsError::NotFound);
            }

            let (resp, code) = self
                .client
                .put_object(prefix_dir.clone(), &[])
                .await
                .unwrap();

            debug!(reason = "creating stub dir file", resp = ?resp, code = code, prefix = %prefix_dir);
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
            let path = S3Backend::normalize_path(path.as_pathbuf());
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
