use std::{
    fmt::Debug,
    sync::{Arc, Mutex}, convert::TryInto, pin::Pin,
};
use bytes::Buf;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20,
};
use futures_util::FutureExt;
use tracing::error;
use webdav_handler::{
    davpath::DavPath,
    fs::{DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, OpenOptions, ReadDirMeta, FsStream, DavDirEntry},
};
use thiserror::Error;

#[derive(Clone)]
pub struct EncryptionWrapper {
    passphrase: [u8; 32],
    nonce: [u8; 12],
    fs: Box<dyn DavFileSystem>,
}

#[derive(Error, std::fmt::Debug)]
pub enum EncryptionError {
    #[error("can't convert to array from slice")]
    FromSlice(#[from] std::array::TryFromSliceError),
}

impl EncryptionWrapper {
    pub fn new(passphrase: &[u8], nonce: &[u8], fs: Box<dyn DavFileSystem>) -> Result<Self, EncryptionError> {
        let passphrase = passphrase.try_into()?;
        let nonce = nonce.try_into()?;
        Ok(EncryptionWrapper {
            passphrase,
            nonce,
            fs,
        })
    }
}

impl DavFileSystem for EncryptionWrapper {
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let file = self.fs.open(path, options).await?;
            Ok(Box::new(EncryptionFileWrapper {
                descriptor: file,
                cipher: Arc::new(Mutex::new(ChaCha20::new(
                    &self.passphrase.into(),
                    &self.nonce.into(),
                ))),
            }) as Box<dyn DavFile>)
        }
        .boxed()
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        self.fs.metadata(path)
    }

    fn read_dir<'a>(&'a self, path: &'a DavPath, rdm: ReadDirMeta) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        self.fs.read_dir(path, rdm)
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        self.fs.create_dir(path)
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        self.fs.remove_file(path)
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        self.fs.remove_dir(path)
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        self.fs.rename(from, to)
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        self.fs.copy(from, to)
    }

    fn have_props<'a>(&'a self, path: &'a DavPath) -> Pin<Box<dyn futures_util::Future<Output = bool> + Send + 'a>> {
        self.fs.have_props(path)
    }

    fn patch_props<'a>(&'a self, path: &'a DavPath, patch: Vec<(bool, webdav_handler::fs::DavProp)>) -> FsFuture<Vec<(hyper::StatusCode, webdav_handler::fs::DavProp)>> {
        self.fs.patch_props(path, patch)
    }

    fn get_prop<'a>(&'a self, path: &'a DavPath, prop: webdav_handler::fs::DavProp) -> FsFuture<Vec<u8>> {
        self.fs.get_prop(path, prop)
    }

    fn get_props<'a>(&'a self, path: &'a DavPath, do_content: bool) -> FsFuture<Vec<webdav_handler::fs::DavProp>> {
        self.fs.get_props(path, do_content)
    }
}

struct EncryptionFileWrapper {
    descriptor: Box<dyn DavFile>,
    cipher: Arc<Mutex<ChaCha20>>,
}

impl Debug for EncryptionFileWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encryption file wrapper")
    }
}

impl DavFile for EncryptionFileWrapper {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        self.descriptor.metadata()
    }

    fn write_buf<'a>(&'a mut self, mut buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        let c = Arc::clone(&self.cipher);
        let mut cipher = match c.lock() {
            Ok(k) => k,
            Err(e) => {
                error!(msg = "can't get cipher lock", err = %e);
                return async move { Err(FsError::GeneralFailure) }.boxed();
            }
        };

        // TODO: optimize?
        let mut v = vec![];
        v.resize(buf.remaining(), 0);
        buf.copy_to_slice(&mut v);
        cipher.apply_keystream(&mut v);
        let b: bytes::Bytes = v.into();
        async move { Ok(self.descriptor.write_buf(Box::new(b)).await?) }.boxed()
    }

    fn write_bytes<'a>(&'a mut self, mut buf: bytes::Bytes) -> FsFuture<()> {
        let c = Arc::clone(&self.cipher);
        let mut cipher = match c.lock() {
            Ok(k) => k,
            Err(e) => {
                error!(msg = "can't get cipher lock", err = %e);
                return async move { Err(FsError::GeneralFailure) }.boxed();
            }
        };

        let mut v = vec![];
        v.resize(buf.len(), 0);
        buf.copy_to_slice(&mut v);
        cipher.apply_keystream(&mut v);
        let b: bytes::Bytes = v.into();
        async move { Ok(self.descriptor.write_bytes(b).await?) }.boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<bytes::Bytes> {
        async move {
            let mut ret_bytes = self.descriptor.read_bytes(count).await?;

            let c = Arc::clone(&self.cipher);
            let mut cipher = match c.lock() {
                Ok(k) => k,
                Err(e) => {
                    error!(msg = "can't get cipher lock", err = %e);
                    return Err(FsError::GeneralFailure);
                }
            };

            let mut v = vec![];
            v.resize(ret_bytes.len(), 0);
            ret_bytes.copy_to_slice(&mut v);
            cipher.apply_keystream(&mut v);
            let b: bytes::Bytes = v.into();
            Ok(b)
        }
        .boxed()
    }

    fn seek<'a>(&'a mut self, s: std::io::SeekFrom) -> FsFuture<u64> {
        self.descriptor.seek(s)
    }

    fn flush<'a>(&'a mut self) -> FsFuture<()> {
        self.descriptor.flush()
    }
}
