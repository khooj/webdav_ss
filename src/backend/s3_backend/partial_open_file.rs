use super::metadata::S3MetaData;
use crate::backend::normalized_path::NormalizedPath;
use bytes::Buf;
use futures_util::FutureExt;
use s3::serde_types::Part;
use s3::Bucket;
use std::io::Write;
use std::time::SystemTime;
use std::{convert::TryInto, io::Cursor};
use tracing::{debug, error, instrument};
use webdav_handler::fs::{DavFile, DavMetaData, FsError, FsFuture, FsResult, OpenOptions};

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct PartialOpenFile {
    path: String,
    options: OpenOptions,
    #[derivative(Debug = "ignore")]
    client: Bucket,
    metadata: S3MetaData,
    etags: Vec<String>,
    upload_id: String,
    cursor: Cursor<Vec<u8>>,
}

impl PartialOpenFile {
    pub async fn new(
        metadata: S3MetaData,
        opts: OpenOptions,
        path: NormalizedPath,
        client: Bucket,
    ) -> FsResult<Self> {
        let (id, code) = match client.create_multipart_upload(path.as_ref()).await {
            Ok(k) => k,
            Err(e) => {
                error!(msg = "can't create multipart upload", err = ?e);
                return Err(FsError::GeneralFailure);
            }
        };

        if code != 200 {
            error!(
                msg = "unsuccessful create multipart upload code",
                code = code
            );
            return Err(FsError::GeneralFailure);
        }

        Ok(PartialOpenFile {
            metadata,
            options: opts,
            path: path.to_string(),
            client,
            etags: vec![],
            upload_id: id.upload_id,
            cursor: Cursor::new(vec![]),
        })
    }
}

const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

impl PartialOpenFile {
    async fn write_chunk<'a, S: bytes::Buf>(&'a mut self, buf: S) -> FsResult<()> {
        let b = buf.chunk();
        let _ = self.cursor.write(b)?;
        if self.cursor.position() < CHUNK_SIZE {
            return Ok(());
        }
        self.upload_current().await
    }

    async fn upload_current<'a>(&'a mut self) -> FsResult<()> {
        self.cursor.set_position(0);

        {
            let b = self.cursor.chunk();
            if b.len() == 0 {
                return Ok(());
            }

            let (resp, code) = match self
                .client
                .upload_part(
                    &self.path,
                    &self.upload_id,
                    (self.etags.len() + 1).try_into().unwrap(),
                    b,
                )
                .await
            {
                Ok(k) => k,
                Err(e) => {
                    error!("{:?}", e);
                    // TODO: retrying?
                    let _ = self
                        .client
                        .abort_multipart_upload(&self.path, &self.upload_id)
                        .await;
                    return Err(FsError::GeneralFailure);
                }
            };

            if code != 200 {
                error!(msg = "can't upload part", code = code);
                let _ = self
                    .client
                    .abort_multipart_upload(&self.path, &self.upload_id)
                    .await;
                return Err(FsError::GeneralFailure);
            }

            self.etags.push(resp);
            self.metadata.len += b.len() as u64;
        }

        self.cursor = Cursor::new(vec![]);
        self.metadata.modified = SystemTime::now();
        Ok(())
    }
}

impl DavFile for PartialOpenFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move { Ok(Box::new(self.metadata.clone()) as Box<dyn DavMetaData>) }.boxed()
    }

    // TODO: do some accumulation if part is lesser than 5MB
    fn write_buf<'a>(&'a mut self, buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        async move { self.write_chunk(buf).await }.boxed()
    }

    fn write_bytes<'a>(&'a mut self, buf: bytes::Bytes) -> FsFuture<()> {
        async move { self.write_chunk(buf).await }.boxed()
    }

    fn read_bytes<'a>(&'a mut self, _: usize) -> FsFuture<bytes::Bytes> {
        async move { Err(FsError::NotImplemented) }.boxed()
    }

    fn seek<'a>(&'a mut self, _: std::io::SeekFrom) -> FsFuture<u64> {
        async move { Err(FsError::NotImplemented) }.boxed()
    }

    #[instrument(level = "debug", skip(self))]
    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        debug!(path = %self.path, length = self.metadata.len);

        async move {
            self.upload_current().await?;

            let parts = self
                .etags
                .clone()
                .into_iter()
                .enumerate()
                .map(|(i, x)| Part {
                    etag: x,
                    part_number: i as u32 + 1,
                })
                .collect::<Vec<Part>>();
            let (_, code) = match self
                .client
                .complete_multipart_upload(&self.path, &self.upload_id, parts)
                .await
            {
                Ok(k) => k,
                Err(e) => {
                    error!(reason = "can't complete multipart upload", err = ?e);
                    return Err(FsError::GeneralFailure);
                }
            };

            if code != 200 {
                error!(reason = "multipart object unsuccessful", code = code);
                let code = match self
                    .client
                    .abort_multipart_upload(&self.path, &self.upload_id)
                    .await
                {
                    Ok(k) => k,
                    Err(e) => {
                        error!("{:?}", e);
                        return Err(FsError::GeneralFailure);
                    }
                };
                if code != 204 {
                    error!(reason = "abort multipart failed", code = code);
                }
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
