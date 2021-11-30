pub mod mem;

use super::normalized_path::NormalizedPath;
use hyper::StatusCode;
use std::future::Future;
use std::pin::Pin;
use webdav_handler::fs::{DavProp, FsError};

type PropResult<T> = Result<T, anyhow::Error>;

type PropFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait PropStorage: Send + Sync + Clone {
    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool>;

    fn patch_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        patch: Vec<(bool, DavProp)>,
    ) -> PropFuture<PropResult<Vec<(StatusCode, DavProp)>>>;

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
}
