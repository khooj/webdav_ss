use super::{PropFuture, PropResult, PropStorage};
use crate::backend::normalized_path::NormalizedPath;
use futures_util::FutureExt;
use hyper::StatusCode;
use webdav_handler::fs::{DavProp, FsError};

#[derive(Clone)]
pub struct Stub;

impl Stub {
    pub fn new() -> Box<dyn PropStorage> {
        Box::new(Stub) as Box<dyn PropStorage>
    }
}

impl PropStorage for Stub {
    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool> {
        async move { false }.boxed()
    }

    fn patch_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        (set, prop): (bool, DavProp),
    ) -> PropFuture<PropResult<(StatusCode, DavProp)>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        prop: DavProp,
    ) -> PropFuture<PropResult<Vec<u8>>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        do_content: bool,
    ) -> PropFuture<PropResult<Vec<DavProp>>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn rename<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }

    fn copy<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        async move { Err(FsError::GeneralFailure) }.boxed()
    }
}
