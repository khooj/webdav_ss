use crate::backend::prop_storages::{mem::Memory, PropStorage};

use super::backend::normalized_path::NormalizedPath;
use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use percent_encoding::{percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
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
pub struct Aggregate {
    filesystems: Routes,
    props: Box<dyn PropStorage>,
}

impl Aggregate {
    pub fn new(props: Box<dyn PropStorage>) -> Self {
        Aggregate {
            filesystems: Routes::new(),
            props,
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
struct AggregateMetaData {
    path: NormalizedPath,
}

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
        async move {
            Ok(Box::new(AggregateMetaData {
                path: self.path.clone(),
            }) as Box<dyn DavMetaData>)
        }
        .boxed()
    }
}

impl DavFileSystem for Aggregate {
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        let span = span!(Level::INFO, "Aggregate::open");
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.open(&path, options).await;
            Ok(result?)
        }
        .instrument(span)
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

        // TODO: should test two cases here:
        // - when all routes mounted at one level like this:
        //      /linode
        //      /minio
        //      /fs
        //      /mem
        // - when some routes mounted inside root that is fs itself:
        //      / (some s3 fs)
        //      /minio
        //      /fs
        //      /mem
        async move {
            let dirs = self.find_routes_at_level(path)?;

            let mut agg_dirs = vec![];
            match self.find_route(&path) {
                Ok((route, path)) => {
                    match route.read_dir(&path, meta).await {
                        Ok(mut result) => {
                            while let Some(i) = result.next().await {
                                agg_dirs.push(i);
                            }
                        },
                        _ => {},
                    }
                },
                _ => {}
            };

            debug!(msg = "generated dirs to output", dirs = ?dirs);
            let ss = stream! {
                for d in agg_dirs {
                    debug!(msg = "yield from route");
                    yield d;
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

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        let span = span!(Level::INFO, "Aggregate::metadata");
        async move {
            let p: NormalizedPath = path.clone().into();
            if p.is_root() {
                return Ok(Box::new(AggregateMetaData { path: p }) as Box<dyn DavMetaData>);
            }
            let (route, path) = self.find_route(&path)?;
            let result = route.metadata(&path).await;
            Ok(result?)
        }
        .instrument(span)
        .boxed()
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "Aggregate::create_dir");
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.create_dir(&path).await;
            Ok(result?)
        }
        .instrument(span)
        .boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "Aggregate::remove_file");
        async move {
            let orig_path = path.clone();
            let (route, path) = self.find_route(&path)?;
            Ok(route
                .remove_file(&path)
                .await
                .and(self.props.remove_file(&orig_path.into()).await)?)
        }
        .instrument(span)
        .boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "Aggregate::remove_dir");
        async move {
            let orig_path = path.clone();
            let (route, path) = self.find_route(&path)?;
            Ok(route
                .remove_dir(&path)
                .await
                .and(self.props.remove_dir(&orig_path.into()).await)?)
        }
        .instrument(span)
        .boxed()
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "Aggregate::rename");
        async move {
            let orig_from = from.clone();
            let orig_to = to.clone();
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route
                .rename(&from, &to)
                .await
                .and(self.props.rename(&orig_from.into(), &orig_to.into()).await)?)
        }
        .instrument(span)
        .boxed()
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let span = span!(Level::INFO, "Aggregate::copy");
        async move {
            let orig_from = from.clone();
            let orig_to = to.clone();
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route
                .copy(&from, &to)
                .await
                .and(self.props.copy(&orig_from.into(), &orig_to.into()).await)?)
        }
        .instrument(span)
        .boxed()
    }

    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { self.props.have_props(&path.into()).await })
    }

    fn patch_props<'a>(
        &'a self,
        path: &'a DavPath,
        patch: Vec<(bool, webdav_handler::fs::DavProp)>,
    ) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        let span = span!(Level::INFO, "Aggregate::patch_props");
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
        .instrument(span)
        .boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a DavPath,
        prop: webdav_handler::fs::DavProp,
    ) -> FsFuture<Vec<u8>> {
        let span = span!(Level::INFO, "Aggregate::get_prop");
        async move { self.props.get_prop(&path.into(), prop).await }
            .instrument(span)
            .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn get_props<'a>(
        &'a self,
        path: &'a DavPath,
        do_content: bool,
    ) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        let span = span!(Level::INFO, "Aggregate::get_props");
        async move { self.props.get_props(&path.into(), do_content).await }
            .instrument(span)
            .boxed()
    }
}

pub struct AggregateBuilder {
    routes: Vec<(String, Box<dyn DavFileSystem>)>,
    props: Box<dyn PropStorage>,
}

impl AggregateBuilder {
    pub fn new() -> Self {
        AggregateBuilder {
            routes: vec![],
            props: Memory::new(),
        }
    }

    pub fn add_route(mut self, (route, fs): (&str, Box<dyn DavFileSystem>)) -> Self {
        self.routes.push((route.to_string(), fs));
        self
    }

    pub fn set_props_storage(mut self, props: Box<dyn PropStorage>) -> Self {
        self.props = props;
        self
    }

    pub fn build(self) -> Result<Box<Aggregate>> {
        let mut agg = Aggregate::new(self.props);
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
        let mut fs = AggregateBuilder::new().build()?;
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

    fn add_route(fs: &mut Box<Aggregate>, route: &str) {
        let _ = fs.add_route((route, MemFs::new()));
    }

    #[test]
    fn check_find_level() -> Result<()> {
        let mut fs = AggregateBuilder::new().build()?;
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
