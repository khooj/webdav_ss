use super::{
    entries::{S3DirEntry, S3File},
    metadata::S3MetaData,
    partial_open_file::PartialOpenFile,
    simple_open_file::S3SimpleOpenFile,
};
use crate::backend::normalized_path::NormalizedPath;
use anyhow::{anyhow, Result};
use futures_util::{FutureExt, StreamExt};
use hyper::StatusCode;
use s3::{
    creds::Credentials,
    region::Region,
    serde_types::{TagSet, Tagging},
    Bucket,
};
use s3::{serde_types::HeadObjectResult, BucketConfiguration};
use std::{io::Cursor, marker::PhantomData};
use tracing::{debug, error, instrument, span, Instrument, Level};
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
    #[instrument(level = "info", err)]
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

    #[instrument(level = "debug", skip(self), err)]
    async fn metadata_info(&self, path: NormalizedPath) -> Result<Box<S3MetaData>, FsError> {
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
            )));
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
        )))
    }

    #[instrument(level = "debug", err, skip(self))]
    async fn read_dir_impl(
        &self,
        path: NormalizedPath,
    ) -> Result<FsStream<Box<dyn DavDirEntry>>, FsError> {
        use async_stream::stream;

        let meta = self.metadata_info(path.clone()).await;
        match meta {
            Err(_) => {
                return Err(FsError::GeneralFailure);
            }
            Ok(k) => {
                if k.is_file() {
                    return Err(FsError::Forbidden);
                }
            }
        };

        let objects = self
            .client
            .list(path.clone().into(), Some("/".into()))
            .await
            .unwrap();

        debug!(msg = "received entries", entries = ?objects);
        let fs = self.clone();
        let s = stream! {
            for e in objects {
                if let Some(v) = e.common_prefixes {
                    for d in v {
                        let m = fs.metadata_info(d.prefix.clone().into()).await;
                        if let Err(_) = m {
                            continue;
                        }
                        let p: NormalizedPath = d.prefix.clone().into();
                        let p = p.strip_prefix(&path);
                        debug!(msg = "generating entry for dir", prefix = ?p);
                        yield Box::new(S3DirEntry {
                            metadata: m.unwrap(),
                            name: p.into(),
                        }) as Box<dyn DavDirEntry>;
                    }
                }

                for c in e.contents {
                    let prefix: NormalizedPath = c.key.into();
                    if prefix.ends_with(".dir") {
                        continue;
                    }
                    let meta = fs.metadata_info(prefix.clone().into()).await;
                    if let Err(_) = meta {
                        debug!(msg = "error metadata for entry", prefix = ?prefix);
                        continue;
                    }
                    let prefix = prefix.strip_prefix(&path);
                    debug!(msg = "generating entry for", prefix = ?prefix);
                    let entry = Box::new(S3DirEntry {
                        metadata: meta.unwrap(),
                        name: prefix.into(),
                    }) as Box<dyn DavDirEntry>;
                    yield entry;
                }
            }
        };

        Ok(Box::pin(s) as FsStream<Box<dyn DavDirEntry>>)
    }

    #[instrument(level = "debug", err, skip(self))]
    async fn remove_file_impl(&self, path: NormalizedPath, dir_check: bool) -> Result<(), FsError> {
        debug!(path = ?path, dir_check = dir_check);
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

        debug!(code = code);
        if code != 204 {
            return Err(FsError::NotFound);
        }

        Ok(())
    }

    #[instrument(level = "debug", err, skip(self))]
    async fn remove_dir_impl(&self, path: NormalizedPath) -> Result<(), FsError> {
        let meta = self.metadata_info(path.clone()).await;
        match meta {
            Err(_) => {
                return Err(FsError::NotFound);
            }
            Ok(k) => {
                if k.is_file() {
                    return Err(FsError::GeneralFailure);
                }
            }
        };

        let dir_file = path.join_file(".dir");
        self.remove_file_impl(dir_file, false).await?;

        Ok(())
    }

    #[instrument(level = "debug", err, skip(self))]
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

            debug!(msg = "creating stub dir file", resp = ?resp, code = code, prefix = ?path);
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

        debug!(msg = "creating stub dir file", resp = ?resp, code = code, prefix = ?prefix_dir);
        if code != 200 {
            return Err(FsError::GeneralFailure);
        }

        Ok(())
    }

    #[instrument(level = "debug", err, skip(self))]
    async fn copy_impl(
        &self,
        mut from: NormalizedPath,
        mut to: NormalizedPath,
    ) -> Result<(), FsError> {
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

    #[instrument(level = "debug", err, skip(self))]
    async fn rename_impl(
        &self,
        from: NormalizedPath,
        mut to: NormalizedPath,
    ) -> Result<(), FsError> {
        if !from.is_collection() && to.is_collection() {
            // TODO: remove files recursive
            if let Ok(_) = self.metadata_info(to.clone()).await {
                self.remove_dir_impl(to.clone()).await?;
            }
            to = to.as_file();
        }

        if !from.is_collection() && !to.is_collection() {
            self.copy_impl(from.clone(), to).await?;
            self.remove_file_impl(from, true).await?;
            return Ok(());
        }

        if from.is_collection() && !to.is_collection() {
            let _ = self.remove_file_impl(to.clone(), true).await;
            to = to.as_dir();
        }

        let mut dirs = vec![from.clone()];
        let mut paths = vec![];
        let mut dirs_to_remove = vec![];
        let mut dirs_to_create = vec![to.clone()];

        while !dirs.is_empty() {
            let path = dirs.pop().unwrap();
            dirs_to_remove.push(path.clone());

            let objects = self
                .read_dir_impl(path.clone().into())
                .await?
                .collect::<Vec<_>>()
                .await;

            for obj in &objects {
                let suffix: NormalizedPath =
                    String::from_utf8_lossy(&obj.name()).to_string().into();
                if obj.is_dir().await? {
                    dirs.push(path.join_dir(&suffix));
                    dirs_to_create.push(to.join_dir(&suffix));
                } else {
                    let to = to.join_file(&suffix);
                    paths.push((path.join_file(&suffix), to))
                }
            }
        }

        for dir in dirs_to_create {
            self.create_dir_impl(dir).await?;
        }

        for (from, to) in paths {
            self.copy_impl(from.clone(), to).await?;
            self.remove_file_impl(from, true).await?;
        }

        for dir in dirs_to_remove {
            self.remove_dir_impl(dir).await?;
        }

        Ok(())
    }
}

impl DavFileSystem for S3Backend {
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        let span = span!(Level::INFO, "S3Backend::open");
        async move {
            let path: NormalizedPath = path.into();
            let meta = self.metadata_info(path.clone()).await;
            if let Ok(k) = meta {
                if k.is_dir() {
                    return Err(FsError::GeneralFailure);
                }
                if options.create_new {
                    return Err(FsError::Exists);
                }
            }

            let mut buf = vec![];
            let (head, code) = self
                .client
                .head_object(path.as_ref())
                .await
                .map_err(|_| FsError::GeneralFailure)?;

            if code == 200 {
                let (obj, code) = self
                    .client
                    .get_object(path.as_ref())
                    .await
                    .map_err(|_| FsError::GeneralFailure)?;

                if code != 200 {
                    error!(msg = "cant get object", code = code);
                    return Err(FsError::GeneralFailure);
                }

                debug!(msg = "received data", length = obj.len());
                buf = obj;
            }

            debug!(is_new = %options.create, path = ?path);
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

            if options.create {
                Ok(Box::new(
                    PartialOpenFile::new(metadata, options, path.into(), self.client.clone())
                        .await?,
                ) as Box<dyn DavFile>)
            } else {
                Ok(Box::new(S3SimpleOpenFile::new(
                    metadata,
                    buf,
                    options,
                    path.into(),
                    self.client.clone(),
                )) as Box<dyn DavFile>)
            }
        }
        .instrument(span)
        .boxed()
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        let span = span!(Level::INFO, "S3Backend::read_dir");
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.read_dir_impl(path).await?)
        }
        .instrument(span)
        .boxed()
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        let span = span!(Level::INFO, "S3Backend::metadata");
        async move { Ok(self.metadata_info(path.into()).await? as Box<dyn DavMetaData>) }
            .instrument(span)
            .boxed()
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "S3Backend::create_dir");
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.create_dir_impl(path).await?)
        }
        .instrument(span)
        .boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "S3Backend::remove_file");
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.remove_file_impl(path, true).await.unwrap())
        }
        .instrument(span)
        .boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "S3Backend::remove_dir");
        async move {
            let path: NormalizedPath = path.into();
            Ok(self.remove_dir_impl(path).await?)
        }
        .instrument(span)
        .boxed()
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "S3Backend::rename");
        async move {
            let from: NormalizedPath = from.into();
            let to: NormalizedPath = to.into();
            Ok(self.rename_impl(from, to).await?)
        }
        .instrument(span)
        .boxed()
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "S3Backend::copy");
        async move {
            let from: NormalizedPath = from.into();
            let to: NormalizedPath = to.into();
            debug!(method = "copy", from = ?from, to = ?to);
            Ok(self.copy_impl(from, to).await?)
        }
        .instrument(span)
        .boxed()
    }

    fn have_props<'a>(
        &'a self,
        _: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        let span = span!(Level::INFO, "S3Backend::have_props");
        async move { true }.instrument(span).boxed()
    }

    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        let span = span!(Level::INFO, "S3Backend::patch_props");
        async move {
            return Err(FsError::NotImplemented);
            let path: NormalizedPath = path.into();
            let mut metadata = self.metadata_info(path.clone()).await?;
            let mut result = vec![];

            debug!(prop = ?patch);
            for (set, p) in patch {
                let pp = p.clone();
                let status = if set {
                    metadata.save_davprop(p);
                    StatusCode::OK
                } else {
                    metadata.remove_davprop(p);
                    StatusCode::OK
                };
                result.push((status, pp));
            }

            let tags = metadata.as_metadata();
            let (_, code) = match self.client.put_object_tagging(&path, &tags[..]).await {
                Err(_) => return Err(FsError::GeneralFailure),
                Ok(k) => k,
            };
            if code != 200 {
                return Err(FsError::GeneralFailure);
            }

            Ok(result)
        }
        .instrument(span)
        .boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        let span = span!(Level::INFO, "S3Backend::get_prop");
        async move {
            return Err(FsError::NotImplemented);
            let path: NormalizedPath = path.into();
            let metadata = self.metadata_info(path).await?;
            Ok(metadata.get_prop(prop).unwrap_or(vec![]))
        }
        .instrument(span)
        .boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        _do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        let span = span!(Level::INFO, "S3Backend::get_prop");
        async move {
            return Err(FsError::NotImplemented);
            let path: NormalizedPath = path.into();
            let metadata = self.metadata_info(path).await?;
            if let Ok(k) = metadata.as_davprops() {
                return Ok(k);
            }
            Err(FsError::GeneralFailure)
        }
        .instrument(span)
        .boxed()
    }
}
