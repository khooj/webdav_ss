use super::repository::Repository;
use anyhow::{anyhow, Result};
use futures_util::FutureExt;
use std::{
    collections::HashMap,
    ffi::OsString,
    sync::{Arc, Mutex},
};
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsFuture, FsStream, OpenOptions,
        ReadDirMeta,
    },
};

type Routes = HashMap<OsString, Box<dyn DavFileSystem>>;

#[derive(Clone)]
pub struct Aggregate {
    filesystems: Routes,
    repository: Arc<Mutex<Box<dyn Repository>>>,
}

impl Aggregate {
    fn new(repository: Box<dyn Repository>) -> Aggregate {
        Aggregate {
            filesystems: Routes::new(),
            repository: Arc::new(Mutex::new(repository)),
        }
    }

    fn add_route(&mut self, (route, fs): (OsString, Box<dyn DavFileSystem>)) -> Result<()> {
        if self.filesystems.contains_key(&route) {
            return Err(anyhow!(
                "aggregate already contains this route: {}",
                route.to_string_lossy()
            ));
        }

        self.filesystems.entry(route).or_insert(fs);
        Ok(())
    }

    fn find_route(&self, route: &DavPath) -> Result<Box<dyn DavFileSystem>> {
        let pb = route.as_pathbuf();
        for p in pb.ancestors() {
            if self.filesystems.contains_key(p.as_os_str()) {
                return Ok(self.filesystems.get(p.as_os_str()).unwrap().clone());
            }
        }
        Err(anyhow!("filesystem not found by route {}", route))
    }
}

// impl DavFileSystem for Aggregate {
//     fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
//         async move {
//             let a: DavPath;
//         }
//         .boxed()
//     }

//     fn read_dir<'a>(
//         &'a self,
//         path: &'a DavPath,
//         meta: ReadDirMeta,
//     ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
//     }

//     fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {}
// }

#[cfg(test)]
mod tests {
    use crate::repository::MemoryRepository;
    use hyper::Uri;
    use std::str::FromStr;
    use webdav_handler::memfs::MemFs;

    use super::*;

    fn helper_path(s: &'static str) -> DavPath {
        DavPath::from_uri(&Uri::from_static(s)).unwrap()
    }

    #[test]
    fn check_find_route() -> Result<()> {
        let mut fs = Aggregate::new(Box::new(MemoryRepository::new()));
        fs.add_route((OsString::from_str("/tmp/fs/fs1").unwrap(), MemFs::new()))?;
        fs.add_route((OsString::from_str("/tmp/fs1").unwrap(), MemFs::new()))?;

        fs.find_route(&helper_path("/tmp/fs/fs1"))?;
        fs.find_route(&helper_path("/tmp/fs1"))?;

        assert!(fs.find_route(&helper_path("/not_exist")).is_err());
        Ok(())
    }
}
