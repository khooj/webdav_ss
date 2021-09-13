use super::repository::Repository;
use anyhow::{anyhow, Result};
use futures_util::{future, FutureExt};
use percent_encoding::percent_encode_byte;
use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

type Routes = HashMap<OsString, Box<dyn DavFileSystem>>;

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

    pub fn add_route(&mut self, (route, fs): (OsString, Box<dyn DavFileSystem>)) -> Result<()> {
        if self.filesystems.contains_key(&route) {
            return Err(anyhow!(
                "aggregate already contains this route: {}",
                route.to_string_lossy()
            ));
        }

        self.filesystems.entry(route).or_insert(fs);
        Ok(())
    }

    fn find_route(&self, route: &DavPath) -> FsResult<(Box<dyn DavFileSystem>, DavPath)> {
        let pb = route.as_pathbuf();
        for p in pb.ancestors() {
            if self.filesystems.contains_key(p.as_os_str()) {
                let path = match pb.strip_prefix(p) {
                    Err(_) => return Err(FsError::NotFound),
                    Ok(k) => Path::new("/").join(k),
                };
                let path = path
                    .to_str()
                    .unwrap()
                    .bytes()
                    .map(percent_encode_byte)
                    .collect::<String>();
                println!("orig: {}, result path: {}", route, path);
                return Ok((
                    self.filesystems.get(p.as_os_str()).unwrap().clone(),
                    DavPath::new(&path).unwrap(),
                ));
            }
        }
        Err(FsError::NotFound)
    }
}

impl DavFileSystem for Aggregate {
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            // if let Err(e) = self.repository.save_file(path, &options) {
            //     return Err(FsError::NotImplemented);
            // }
            let (route, path) = self.find_route(&path)?;
            Ok(route.open(&path, options).await?)
        }
        .boxed()
    }

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

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.metadata(&path).await?)
        }
        .boxed()
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.create_dir(&path).await?)
        }
        .boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.remove_file(&path).await?)
        }
        .boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.remove_dir(&path).await?)
        }
        .boxed()
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, from) = self.find_route(&from)?;
            let (_, to) = self.find_route(&to)?;
            Ok(route.rename(&from, &to).await?)
        }
        .boxed()
    }

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
        future::ready(true).boxed()
    }

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
        self.agg.add_route((OsString::from_str(route)?, fs))?;
        Ok(self)
    }

    pub fn build(self) -> Box<Aggregate> {
        Box::new(self.agg)
    }
}

#[cfg(test)]
mod tests {
    use crate::repository::MemoryRepository;
    use hyper::Uri;
    use std::str::FromStr;
    use webdav_handler::memfs::MemFs;

    use super::*;

    fn helper_path(s: &'static str) -> DavPath {
        // let ss =
        //     percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC)
        //         .to_string();
        let ss = s.parse::<Uri>().unwrap();
        println!("{}", ss);
        DavPath::new(&s).unwrap()
    }

    #[test]
    fn check_find_route() -> Result<()> {
        let mut fs = Aggregate::new(Box::new(MemoryRepository::new()));
        fs.add_route((OsString::from_str("/tmp/fs/fs1").unwrap(), MemFs::new()))?;
        fs.add_route((OsString::from_str("/tmp/fs1").unwrap(), MemFs::new()))?;

        let (_, f) = fs.find_route(&helper_path("/tmp/fs/fs1"))?;
        assert_eq!(f, helper_path("/"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1"))?;
        assert_eq!(f, helper_path("/"));

        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/fs1"))?;
        assert_eq!(f, helper_path("/fs1"));

        // /res-€
        let (_, f) = fs.find_route(&helper_path("/tmp/fs1/res-%e2%82%ac"))?;
        assert_eq!(f, helper_path("/res-%e2%82%ac"));

        assert!(fs.find_route(&helper_path("/not_exist")).is_err());
        Ok(())
    }
}
