use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use tracing::debug;
use webdav_handler::fs::{DavMetaData, FsResult};

#[derive(derivative::Derivative)]
#[derivative(Debug, Clone, Default)]
pub struct S3MetaData {
    path: String,
    len: u64,
    #[derivative(Default(value = "SystemTime::now()"))]
    modified: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    created: SystemTime,
    executable: bool,
    is_dir: bool,
    etag: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct PropName {
    name: String,
    prefix: Option<String>,
    namespace: Option<String>,
}
#[derive(Serialize, Deserialize, Default)]
struct Prop {
    pub value: Option<Vec<u8>>,
}

impl S3MetaData {
    pub fn extract_from_tags(
        len: u64,
        path: String,
        is_dir: bool,
        etag: Option<String>,
        modified: Option<String>,
    ) -> Self {
        use std::convert::TryInto;

        let m = DateTime::parse_from_rfc2822(&modified.unwrap_or(String::new())).map(|dt| {
            std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(dt.timestamp().try_into().unwrap())
        });
        let mut metadata = S3MetaData::default();
        metadata.len = len;
        metadata.path = path;
        metadata.is_dir = is_dir;
        metadata.etag = etag;
        metadata.modified = m.unwrap_or(SystemTime::now());

        metadata
    }

    fn as_unixtime(t: SystemTime) -> String {
        if let Ok(n) = t.duration_since(std::time::UNIX_EPOCH) {
            n.as_secs().to_string()
        } else {
            "0".to_owned()
        }
    }

    pub fn as_metadata(&self) -> Vec<(String, String)> {
        let modified = S3MetaData::as_unixtime(self.modified);
        let created = S3MetaData::as_unixtime(self.created);
        let result = vec![("modified".into(), modified), ("created".into(), created)];
        result
    }

    pub fn add_len(&mut self, s: u64) {
        self.len += s;
    }

    pub fn modified_now(&mut self) {
        self.modified = SystemTime::now();
    }

    pub fn len(&self) -> u64 {
        self.len
    }
}

impl DavMetaData for S3MetaData {
    fn len(&self) -> u64 {
        self.len
    }

    fn modified(&self) -> FsResult<SystemTime> {
        Ok(self.modified)
    }

    fn is_dir(&self) -> bool {
        self.is_dir
    }

    fn created(&self) -> FsResult<SystemTime> {
        Ok(self.created)
    }

    fn executable(&self) -> FsResult<bool> {
        Ok(self.executable)
    }

    fn etag(&self) -> Option<String> {
        if let Ok(t) = self.modified() {
            if let Ok(t) = t.duration_since(std::time::UNIX_EPOCH) {
                let t = t.as_secs() * 1000000 + t.subsec_nanos() as u64 / 1000;
                let tag = if self.is_file() && self.len() > 0 {
                    format!("{:x}-{:x}", self.len(), t)
                } else {
                    format!("{:x}", t)
                };
                return Some(tag);
            }
        }
        None
    }
}
