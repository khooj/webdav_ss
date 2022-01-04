use bincode::{deserialize, serialize};
use s3::serde_types::Tagging;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};
use webdav_handler::fs::{DavMetaData, DavProp, FsResult};

#[derive(derivative::Derivative)]
#[derivative(Debug, Clone, Default)]
pub struct S3MetaData {
    pub path: String,
    pub len: u64,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub modified: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub created: SystemTime,
    pub executable: bool,
    pub is_dir: bool,
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
    fn extract_unixtime_or_zero(value: &str) -> SystemTime {
        if let Ok(k) = value.parse() {
            std::time::UNIX_EPOCH + Duration::from_secs(k)
        } else {
            SystemTime::now()
        }
    }

    pub fn extract_from_tags(len: u64, path: String, is_dir: bool) -> Self {
        let mut metadata = S3MetaData::default();
        metadata.len = len;
        metadata.path = path;
        metadata.is_dir = is_dir;

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
        let mut result = vec![("modified".into(), modified), ("created".into(), created)];
        result
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
}
