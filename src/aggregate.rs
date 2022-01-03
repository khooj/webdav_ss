use crate::{
    backend::prop_storages::{mem::Memory, PropStorage},
    configuration::PropsStorage,
    repository::MemoryRepository,
};

use super::backend::normalized_path::NormalizedPath;
use super::repository::Repository;
use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use percent_encoding::{percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
};
use tracing::{debug, instrument, span, Instrument, Level};
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

type Routes = HashMap<String, Box<dyn DavFileSystem>>;

const ENC: &AsciiSet = &NON_ALPHANUMERIC.remove(b'.').remove(b'/').remove(b'"');

#[derive(Clone)]
pub struct Aggregate<T> {
    filesystems: Routes,
    props: T,
    repository: Arc<Mutex<Box<dyn Repository>>>,
}

impl<T> Aggregate<T> {
    pub fn new(repository: Box<dyn Repository>, props: T) -> Self {
        Aggregate {
            filesystems: Routes::new(),
            props,
            repository: Arc::new(Mutex::new(repository)),
        }
    }

    #[instrument(level = "debug", err, skip(self, fs))]
    pub fn add_route(&mut self, (route, fs): (&str, Box<dyn DavFileSystem>)) -> Result<()> {
        let route = route.into();
        if self.filesystems.contains_key(&route) {
            return Err(anyhow!("aggregate already contains this route: {}", route,));
        }

        self.filesystems.entry(route).or_insert(fs);
        Ok(())
    }

    #[instrument(level = "debug", err, skip(self))]
    fn find_route(&self, route: &DavPath) -> FsResult<(Box<dyn DavFileSystem>, DavPath)> {
        let col = route.is_collection();
        let pb = route.as_pathbuf();
        for p in pb.ancestors() {
            let p = p.to_str().ok_or(FsError::NotFound)?.to_owned();
            if self.filesystems.contains_key(&p) {
                let path = match pb.strip_prefix(p.clone()) {
                    Err(_) => return Err(FsError::NotFound),
                    Ok(k) => k,
                };
                let mut path = format!(
                    "/{}",
                    percent_encode(path.to_str().unwrap().as_bytes(), ENC).to_owned()
                );
                if col && !path.ends_with('/') {
                    path = format!("{}/", path);
                }
                debug!(route = %p, path = %path);
                return Ok((
                    self.filesystems.get(&p).unwrap().clone(),
                    DavPath::new(&path).unwrap(),
                ));
            }
        }
        Err(FsError::NotFound)
    }

    // TODO: rewrite method with better code.
    #[instrument(level = "debug", skip(self))]
    fn find_routes_at_level(&self, level: &DavPath) -> FsResult<Vec<String>> {
        let mut results = HashSet::new();

        let col = level.is_collection();
        let mut level = level.as_pathbuf();
        if col {
            // append something for parent() call
            level = level.join("e");
        }
        let level = level.parent().unwrap_or(&Path::new("/"));
        let level = level.to_str().ok_or(FsError::NotFound)?.to_owned();
        for (k, _) in &self.filesystems {
            let p = Path::new(k);
            let stripped = p.strip_prefix(&level);
            if let Ok(k) = stripped {
                let k = k.to_str().unwrap();
                if k.is_empty() {
                    continue;
                }
                let el = k.split('/').nth(0);
                if el.is_none() {
                    continue;
                }

                let el = el.unwrap();
                let l = Path::new(&level);
                let pp = l.join(el);

                results.insert(pp.to_str().unwrap().to_owned());
            }
        }

        Ok(results.into_iter().collect())
    }
}

#[derive(Debug, Clone)]
struct AggregateMetaData {}

impl DavMetaData for AggregateMetaData {
    fn len(&self) -> u64 {
        4 * 1024
    }

    fn created(&self) -> FsResult<std::time::SystemTime> {
        Ok(std::time::SystemTime::now())
    }

    fn modified(&self) -> FsResult<std::time::SystemTime> {
        Ok(std::time::SystemTime::now())
    }

    fn is_dir(&self) -> bool {
        true
    }
}

struct AggregateDirEntry {
    path: NormalizedPath,
}

impl DavDirEntry for AggregateDirEntry {
    fn name(&self) -> Vec<u8> {
        self.path.clone().into()
    }

    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(Box::new(AggregateMetaData {}) as Box<dyn DavMetaData>) }.boxed()
    }
}

impl<T> DavFileSystem for Aggregate<T>
where
    T: PropStorage + 'static,
{
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.open(&path, options).await;
            Ok(result?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        meta: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        use async_stream::stream;
        use futures_util::StreamExt;
        let span = span!(Level::INFO, "Aggregate::read_dir");

        async move {
            let dirs = self.find_routes_at_level(path)?;

            debug!(msg = "generated dirs to output", dirs = ?dirs);
            let (route, path) = self.find_route(&path)?;
            let mut result = route.read_dir(&path, meta).await?;
            let ss = stream! {
                while let Some(i) = result.next().await {
                    debug!(msg = "output from route()");
                    yield i;
                }
                for d in dirs {
                    debug!(msg = "yield aggregate dirs", name = %d);
                    yield Box::new(AggregateDirEntry{ path: format!("{}/", d).into() }) as Box<dyn DavDirEntry>;
                }
            };
            Ok(Box::pin(ss) as FsStream<Box<dyn DavDirEntry>>)
        }
        .instrument(span)
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.metadata(&path).await;
            Ok(result?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.create_dir(&path).await;
            Ok(result?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route
                .remove_file(&path)
                .await
                .and(self.props.remove_file(&path.into()).await)?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route
                .remove_dir(&path)
                .await
                .and(self.props.remove_dir(&path.into()).await)?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route
                .rename(&from, &to)
                .await
                .and(self.props.rename(&from.into(), &to.into()).await)?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route
                .copy(&from, &to)
                .await
                .and(self.props.copy(&from.into(), &to.into()).await)?)
        }
        .boxed()
    }

    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { self.props.have_props(&path.into()).await })
    }

    #[instrument(level = "debug", skip(self))]
    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        async move {
            let mut r = vec![];
            let path: NormalizedPath = path.into();
            for (set, prop) in patch {
                let pr = self
                    .props
                    .patch_prop(&path, (set, prop))
                    .await
                    .map_err(|_| FsError::GeneralFailure)?;
                r.push(pr);
            }
            Ok(r)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        async move { self.props.get_prop(&path.into(), prop).await }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        async move { self.props.get_props(&path.into(), do_content).await }.boxed()
    }
}

pub struct AggregateBuilder {
    routes: Vec<(String, Box<dyn DavFileSystem>)>,
}

impl AggregateBuilder {
    pub fn new() -> Self {
        AggregateBuilder { routes: vec![] }
    }

    pub fn add_route(mut self, (route, fs): (&str, Box<dyn DavFileSystem>)) -> Self {
        self.routes.push((route.to_string(), fs));
        self
    }

    pub fn build<T>(self, prop_storage: T) -> Result<Box<Aggregate<T>>> {
        let mut agg = Aggregate::new(Box::new(MemoryRepository::new()), prop_storage);
        for (route, fs) in self.routes {
            agg.add_route((&route, fs))?;
        }
        Ok(Box::new(agg))
    }
}

#[cfg(test)]
mod tests {
    use webdav_handler::{davpath::DavPath, memfs::MemFs};

    use super::*;

    fn helper_path(s: &'static str) -> DavPath {
        DavPath::new(&s).unwrap()
    }

    #[test]
    fn check_find_route() -> Result<()> {
        let mut fs = AggregateBuilder::new().build(Memory::new())?;
        fs.add_route(("/tmp/fs/fs1", MemFs::new()))?;
        fs.add_route(("/tmp/fs1", MemFs::new()))?;

        let (_, f) = fs.find_route(&helper_path("/tmp/fs/fs1"))?;
        assert_eq!(f, helper_path("/"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1"))?;
        assert_eq!(f, helper_path("/"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/fs1"))?;
        assert_eq!(f, helper_path("/fs1"));

        // /res-€
        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/res-%e2%82%ac"))?;
        assert_eq!(f, helper_path("/res-%e2%82%ac"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/one/two"))?;
        assert_eq!(f, helper_path("/one/two"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/one/two.txt"))?;
        assert_eq!(f, helper_path("/one/two.txt"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/one/"))?;
        assert_eq!(f, helper_path("/one/"));

        assert!(fs.find_route(&helper_path("/not_exist")).is_err());
        Ok(())
    }

    fn add_route<T>(fs: &mut Box<Aggregate<T>>, route: &str) {
        let _ = fs.add_route((route, MemFs::new()));
    }

    #[test]
    fn check_find_level() -> Result<()> {
        let mut fs = AggregateBuilder::new().build(Memory::new())?;
        add_route(&mut fs, "/fs1");
        add_route(&mut fs, "/fs2");
        add_route(&mut fs, "/tmp/fs1");
        add_route(&mut fs, "/tmp/fs2");
        add_route(&mut fs, "/tmp/tmp/fs2");
        add_route(&mut fs, "/tmp/tmp/tmp/fs2");

        assert_eq!(fs.find_routes_at_level(&helper_path("/"))?.len(), 3);
        assert_eq!(fs.find_routes_at_level(&helper_path("/fs1/"))?.len(), 0);
        assert_eq!(fs.find_routes_at_level(&helper_path("/tmp/"))?.len(), 3);
        assert_eq!(fs.find_routes_at_level(&helper_path("/tmp/tmp/"))?.len(), 2);
        assert_eq!(
            fs.find_routes_at_level(&helper_path("/tmp/tmp/tmp/"))?
                .len(),
            1
        );

        Ok(())
    }
}
