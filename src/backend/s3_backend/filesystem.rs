use super::{
    entries::{S3DirEntry, S3OpenFile},
    metadata::S3MetaData,
    normalized_path::NormalizedPath,
};
use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use s3::{
    creds::Credentials,
    region::Region,
    serde_types::{ListBucketResult, TagSet, Tagging},
    Bucket, S3Error,
};
use s3::{serde_types::HeadObjectResult, BucketConfiguration};
use std::io::Cursor;
use tracing::{debug, error, instrument};
use webdav_handler::memfs::MemFs;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsStream, OpenOptions,
        ReadDirMeta,
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

    #[instrument(skip(self))]
    async fn metadata_info(&self, path: NormalizedPath) -> Result<Box<dyn DavMetaData>, FsError> {
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

        let mut is_col = false;
        let mut head: Option<(HeadObjectResult, NormalizedPath)> = None;
        // check if it dir or file
        for prefix in [path.join_file(".dir"), path.clone()] {
            let (resp, code) = self.client.head_object(prefix.clone()).await.unwrap();
            if code != 200 {
                continue;
            }
            if prefix.ends_with(".dir") {
                is_col = true;
            }
            head = Some((resp, prefix));
            break;
        }

        if head.is_none() {
            debug!(msg = "not found", path = ?path);
            return Err(FsError::NotFound);
        }

        let head = head.unwrap();

        if !is_col {
            let (t, code) = self.client.get_object_tagging(head.1).await.unwrap();
            tags = t;

            if code != 200 {
                tags = Some(Tagging {
                    tag_set: TagSet { tags: vec![] },
                });
            }
        }

        let len = head.0.content_length.unwrap_or(0i64) as u64;
        Ok(Box::new(S3MetaData::extract_from_tags(
            len,
            path.into(),
            tags.unwrap(),
            is_col,
        )) as Box<dyn DavMetaData>)
    }

    #[instrument(skip(self))]
    async fn list_objects(&self, path: NormalizedPath) -> Result<Vec<ListBucketResult>, S3Error> {
        let mut prefix = path;
        if prefix.len() > 1 && !prefix.ends_with("/") {
            prefix.push('/');
        }
        let result = self.client.list(prefix.into(), Some("".into())).await?;
        let result = result
            .into_iter()
            .map(|mut f| {
                f.contents = f
                    .contents
                    .into_iter()
                    .filter(|k| !k.key.ends_with(".dir"))
                    .collect();
                f
            })
            .collect();
        Ok(result)
    }

    async fn remove_file_impl(&self, path: NormalizedPath, dir_check: bool) -> Result<(), FsError> {
        debug!(method = "remove file", path = ?path, dir_check = dir_check);
        let meta = self.metadata_info(path.clone()).await;
        match meta {
            Err(_) => {
                return Err(FsError::NotFound);
            }
            Ok(k) => {
                if k.is_dir() && dir_check {
                    return Err(FsError::GeneralFailure);
                }
            }
        };
        let (_, code) = self.client.delete_object(path.as_ref()).await.unwrap();

        debug!(method = "remove file", code = code);
        if code != 204 {
            return Err(FsError::NotFound);
        }

        Ok(())
    }

    async fn remove_dir_impl(&self, path: NormalizedPath) -> Result<(), FsError> {
        let meta = self.metadata_info(path.clone()).await;
        match meta {
            Err(e) => {
                return Err(FsError::NotFound);
            }
            Ok(k) => {
                if k.is_file() {
                    return Err(FsError::GeneralFailure);
                }
            }
        };

        let objects = self.list_objects(path.clone()).await.unwrap();

        for obj in objects
            .into_iter()
            .filter(|p| p.prefix == path.as_str())
            .flat_map(|f| f.contents)
        {
            self.remove_file_impl(obj.key.clone().into(), true).await?;
        }

        let dir_file = path.join_file(".dir");
        self.remove_file_impl(dir_file, false).await?;

        Ok(())
    }

    async fn create_dir_impl(&self, path: NormalizedPath) -> Result<(), FsError> {
        let meta = self.metadata_info(path.clone()).await;
        if let Ok(m) = meta {
            if m.is_dir() {
                debug!(msg = "dir already exist", path = ?path);
                return Err(FsError::Exists);
            }
        }

        let prefix_dir = path.join_file(".dir");
        if path.ends_with("/") && path.starts_with("/") {
            let (resp, code) = self
                .client
                .put_object(prefix_dir.clone(), &[])
                .await
                .unwrap();

            debug!(reason = "creating stub dir file", resp = ?resp, code = code, prefix = ?path);
            if code != 200 {
                return Err(FsError::GeneralFailure);
            }
            return Ok(());
        }

        // let pb = prefix_dir.as_pathbuf();
        let parent = prefix_dir.dirs_parent();
        let meta = self.metadata_info(parent.clone().into()).await;
        match meta {
            Err(e) => {
                debug!(msg = "parent folder does not exist", parent = ?parent, err = %e);
                return Err(FsError::NotFound);
            }
            Ok(k) => {
                if k.is_file() {
                    debug!(msg = "tried to create subfolder in file", parent = ?parent);
                    return Err(FsError::Forbidden);
                }
            }
        };

        let (resp, code) = self
            .client
            .put_object(prefix_dir.clone(), &[])
            .await
            .unwrap();

        debug!(reason = "creating stub dir file", resp = ?resp, code = code, prefix = ?prefix_dir);
        if code != 200 {
            return Err(FsError::GeneralFailure);
        }

        Ok(())
    }
}

impl DavFileSystem for S3Backend {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let path: NormalizedPath = path.into();
            let meta = self.metadata_info(path.clone()).await;
            if let Ok(k) = meta {
                if k.is_dir() {
                    return Err(FsError::GeneralFailure);
                }
            }
            let mut is_new = false;
            let mut buf = vec![];
            let (head, code) = self
                .client
                .head_object(path.as_ref())
                .await
                .map_err(|_| FsError::GeneralFailure)?;

            if code != 200 {
                is_new = true;
            } else {
                let (obj, code) = self
                    .client
                    .get_object(path.as_ref())
                    .await
                    .map_err(|_| FsError::GeneralFailure)?;

                if code != 200 {
                    error!(reason = "cant get object", code = code);
                    return Err(FsError::GeneralFailure);
                }

                debug!(reason = "received data", length = obj.len());
                buf = obj;
            }

            debug!(is_new = %is_new, path = ?path);
            let (mut tags, code) = self.client.get_object_tagging(path.as_ref()).await.unwrap();

            if code != 200 {
                tags = Some(Tagging {
                    tag_set: TagSet { tags: vec![] },
                });
            }

            debug!(tags = ?tags);
            let len = head.content_length.unwrap_or(0i64) as u64;
            let metadata = S3MetaData::extract_from_tags(
                len,
                path.clone().into(),
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
                path: path.into(),
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
        use futures_util::{stream, FutureExt};
        async move {
            let path: NormalizedPath = path.into();
            let meta = self.metadata_info(path.clone()).await;
            match meta {
                Err(e) => {
                    return Err(FsError::GeneralFailure);
                }
                Ok(k) => {
                    if k.is_file() {
                        return Err(FsError::Forbidden);
                    }
                }
            };

            let objects = self.list_objects(path.clone()).await.unwrap();

            debug!(method = "read_dir", msg = "received entries", entries = ?objects);
            let mut entries = vec![];
            for e in objects {
                for c in e.contents {
                    let prefix: NormalizedPath = c.key.into();
                    let meta = self.metadata_info(prefix.clone().into()).await;
                    if let Err(e) = meta {
                        continue;
                    }
                    let prefix = prefix.split_prefix(&path);
                    let entry = Box::new(S3DirEntry {
                        metadata: meta.unwrap(),
                        name: prefix.into(),
                    }) as Box<dyn DavDirEntry>;
                    entries.push(entry);
                }
            }

            let s = stream::iter(entries);
            let s = Box::pin(s) as FsStream<Box<dyn DavDirEntry>>;

            Ok(s)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(self.metadata_info(path.into()).await?) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.create_dir_impl(path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.remove_file_impl(path, true).await.unwrap())
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.remove_dir_impl(path).await?)
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
            let mut from: NormalizedPath = from.into();
            let mut to: NormalizedPath = to.into();
            debug!(method = "copy", from = ?from, to = ?to);

            // let from_meta = self.metadata_info(from.clone()).await?;
            let to_meta = self.metadata_info(to.clone()).await;

            if let Err(_) = to_meta {
                if !from.is_collection() && to.is_collection() {
                    to = to.as_file();
                }
            }

            if from.is_collection() && to.is_collection() {
                from = from.join_file(".dir");
                to = to.join_file(".dir");
            }

            if let Err(_) = self.metadata_info(to.parent()).await {
                self.create_dir_impl(to.parent()).await?;
            }

            let (_, code) = self
                .client
                .copy_object(from.into(), to.into())
                .await
                .unwrap();

            if code != 200 {
                return Err(FsError::GeneralFailure);
            }

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
