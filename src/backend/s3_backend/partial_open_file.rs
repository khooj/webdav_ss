use super::metadata::S3MetaData;
use crate::backend::normalized_path::NormalizedPath;
use bytes::Buf;
use futures_util::FutureExt;
use s3::Bucket;
use std::io::{Cursor, SeekFrom};
use std::time::SystemTime;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{debug, instrument};
use webdav_handler::fs::{DavFile, DavMetaData, FsError, FsFuture, OpenOptions};

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct PartialOpenFile {
    is_new: bool,
    path: String,
    options: OpenOptions,
    cursor: Cursor<Vec<u8>>,
    #[derivative(Debug = "ignore")]
    client: Bucket,
    metadata: S3MetaData,
}

impl PartialOpenFile {
    pub fn new(
        metadata: S3MetaData,
        buf: Vec<u8>,
        new: bool,
        opts: OpenOptions,
        path: NormalizedPath,
        client: Bucket,
    ) -> Self {
        PartialOpenFile {
            metadata,
            cursor: Cursor::new(buf),
            is_new: new,
            options: opts,
            path: path.to_string(),
            client,
        }
    }
}

impl DavFile for PartialOpenFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(Box::new(self.metadata.clone()) as Box<dyn DavMetaData>) }.boxed()
    }

    fn write_buf<'a>(&'a mut self, buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        async move {
            let b = buf.chunk();
            self.cursor.write(b).await.unwrap();
            self.metadata.modified = SystemTime::now();
            self.metadata.len += b.len() as u64;
            Ok(())
        }
        .boxed()
    }

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {
        async move {
            self.cursor.write(buf.chunk()).await.unwrap();
            self.metadata.modified = SystemTime::now();
            self.metadata.len += buf.len() as u64;
            Ok(())
        }
        .boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {
        async move {
            let mut b = Vec::with_capacity(count);
            b.resize(count, 0);
            self.cursor.read(b.as_mut()).await.unwrap();
            Ok(bytes::Bytes::from(b))
        }
        .boxed()
    }

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<u64> {
        async move { Ok(self.cursor.seek(pos).await.unwrap()) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        let mut data = self.cursor.clone();
        debug!(path = %self.path, length = self.metadata.len);

        async move {
            data.seek(SeekFrom::Start(0)).await.unwrap();
            let (_, code) = self
                .client
                .put_object(self.path.to_string(), data.chunk())
                .await
                .unwrap();

            if code != 200 {
                debug!(reason = "put object unsuccessful", code = code);
                return Err(FsError::GeneralFailure);
            }

            let tags = self.metadata.as_metadata();

            let (_, code) = self
                .client
                .put_object_tagging(&self.path.to_string(), &tags[..])
                .await
                .unwrap();
            if code != 200 {
                debug!(reason = "tag object unsuccessful", code = code);
                return Err(FsError::GeneralFailure);
            }
            Ok(())
        }
        .boxed()
    }
}
