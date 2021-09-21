use anyhow::Result;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::S3Error;
use tracing::instrument;
use webdav_handler::{
    davpath::DavPath,
    fs::{
        DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
        OpenOptions, ReadDirMeta,
    },
};

pub struct S3 {
    name: String,
    region: Region,
    credentials: Credentials,
    bucket: Bucket,
}

impl S3 {
    pub fn new(name: &str, region: &str, bucket: &str) -> Result<Self> {
        let name = name.to_owned();
        let region = region.parse()?;
        let creds = Credentials::from_profile(Some("minio"))?;
        let bucket = Bucket::new(bucket, region, creds)?;

        Ok(S3 {
            name,
            region,
            credentials: creds,
            bucket,
        })
    }
}

impl DavFileSystem for S3 {
    #[instrument(level = "debug", skip(self))]
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            self.bucket.put_object_stream(path, s3_path)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
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

    #[instrument(level = "debug", skip(self))]
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.metadata(&path).await?)
        }
        .boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        async move {
            let (route, path) = self.find_route(&path)?;
            Ok(route.create_dir(&path).await?)
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

    #[instrument(level = "debug", skip(self))]
    fn have_props<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> std::pin::Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        future::ready(true).boxed()
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

fn check_bucket() -> Result<()> {
    let creds = Credentials::from_profile(Some("minio"))?;
    let bucket = Bucket::new("test", "eu-central-1".parse()?, creds)?;
}

#[cfg(test)]
mod tests {}
