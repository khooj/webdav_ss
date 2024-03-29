pub mod mem;
pub mod yaml;
pub mod kv;
pub mod stub;

use super::normalized_path::NormalizedPath;
use hyper::StatusCode;
use std::future::Future;
use std::pin::Pin;
use webdav_handler::fs::{DavProp, FsError};

type PropResult<T> = Result<T, FsError>;

type PropFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait BoxClonePropStorage {
    fn box_clone(&self) -> Box<dyn PropStorage>;
}

impl Clone for Box<dyn PropStorage> {
    fn clone(&self) -> Box<dyn PropStorage> {
        self.box_clone()
    }
}

impl<Storage: Clone + PropStorage + 'static> BoxClonePropStorage for Storage {
    fn box_clone(&self) -> Box<dyn PropStorage> {
        Box::new((*self).clone())
    }
}

pub trait PropStorage: Send + Sync + BoxClonePropStorage {
    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool>;

    fn patch_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        patch: (bool, DavProp),
    ) -> PropFuture<PropResult<(StatusCode, DavProp)>>;

    fn get_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        prop: DavProp,
    ) -> PropFuture<PropResult<Vec<u8>>>;

    fn get_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        do_content: bool,
    ) -> PropFuture<PropResult<Vec<DavProp>>>;

    fn remove_file<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>>;
    fn remove_dir<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>>;
    fn rename<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>>;
    fn copy<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>>;
}
