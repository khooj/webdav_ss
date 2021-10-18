use super::repository::Repository;
use anyhow::{anyhow, Result};
use futures_util::{future, FutureExt};
use percent_encoding::{percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
};
use tracing::{debug, instrument};
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
    repository: Arc<Mutex<Box<dyn Repository>>>,
}

impl Aggregate {
    pub fn new(repository: Box<dyn Repository>) -> Aggregate {
        Aggregate {
            filesystems: Routes::new(),
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
                if col {
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

    #[instrument(level = "debug", skip(self))]
    fn find_routes_at_level(&self, level: &DavPath) -> FsResult<Vec<String>> {
        let mut results = HashSet::new();

        let level = level.as_pathbuf();
        let level = level.parent().unwrap_or(&Path::new("/"));
        let level = level.to_str().ok_or(FsError::NotFound)?.to_owned();
        for (k, v) in &self.filesystems {
            let kk = Path::new(k);
            let kk = kk.parent();
            if kk.is_none() {
                continue;
            }
            let kk = kk.unwrap();
            let kk = kk.to_str();
            if kk.is_none() {
                continue;
            }
            let kk = kk.unwrap();

            if level == kk || kk.starts_with(&level) {
                results.insert(kk.clone());
            }
        }
        Ok(results.into_iter().collect())
    }
}

impl DavFileSystem for Aggregate {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.open(&path, options).await;
            debug!(method = "open", result = ?result);
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
        use futures_util::StreamExt;

        async move {
            let (route, path) = self.find_route(&path)?;
            let result = route.read_dir(&path, meta).await;
            Ok(result?)
        }
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

    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            if let Ok((route, path)) = self.find_route(path) {
                route.have_props(&path).await
            } else {
                false
            }
        })
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

pub struct AggregateBuilder {
    agg: Aggregate,
}

impl AggregateBuilder {
    pub fn new(repo: Box<dyn Repository>) -> AggregateBuilder {
        AggregateBuilder {
            agg: Aggregate::new(repo),
        }
    }

    pub fn add_route(
        mut self,
        (route, fs): (&str, Box<dyn DavFileSystem>),
    ) -> Result<AggregateBuilder> {
        self.agg.add_route((route, fs))?;
        Ok(self)
    }

    pub fn build(self) -> Box<Aggregate> {
        Box::new(self.agg)
    }
}

#[cfg(test)]
mod tests {
    use crate::repository::MemoryRepository;
    use webdav_handler::{davpath::DavPath, memfs::MemFs};

    use super::*;

    fn helper_path(s: &'static str) -> DavPath {
        DavPath::new(&s).unwrap()
    }

    #[test]
    fn check_find_route() -> Result<()> {
        let mut fs = Aggregate::new(Box::new(MemoryRepository::new()));
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

    fn add_route(fs: &mut Aggregate, route: &str) {
        fs.add_route((route, MemFs::new()));
    }

    #[test]
    fn check_find_level() -> Result<()> {
        let mut fs = Aggregate::new(Box::new(MemoryRepository::new()));
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
