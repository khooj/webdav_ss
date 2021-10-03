use s3::serde_types::Tagging;
use std::time::{Duration, SystemTime};
use webdav_handler::fs::{DavMetaData, FsResult};

#[derive(derivative::Derivative)]
#[derivative(Debug, Clone, Default)]
pub struct S3MetaData {
    pub path: String,
    pub len: u64,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub modified: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub accessed: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub created: SystemTime,
    #[derivative(Default(value = "SystemTime::now()"))]
    pub status_changed: SystemTime,
    pub executable: bool,
    pub is_dir: bool,
}

impl S3MetaData {
    fn extract_unixtime_or_zero(value: &str) -> SystemTime {
        if let Ok(k) = value.parse() {
            std::time::UNIX_EPOCH + Duration::from_secs(k)
        } else {
            SystemTime::now()
        }
    }

    pub fn extract_from_tags(len: u64, path: String, tags: Tagging, is_dir: bool) -> Self {
        let mut metadata = S3MetaData::default();
        metadata.len = len;
        metadata.path = path;
        metadata.is_dir = is_dir;

        for kv in tags.tag_set.tags.into_iter() {
            let v = kv.value();
            match &kv.key().as_str() {
                &"modified" => metadata.modified = S3MetaData::extract_unixtime_or_zero(&v),
                &"accessed" => metadata.accessed = S3MetaData::extract_unixtime_or_zero(&v),
                &"created" => metadata.created = S3MetaData::extract_unixtime_or_zero(&v),
                &"status_changed" => {
                    metadata.status_changed = S3MetaData::extract_unixtime_or_zero(&v)
                }
                _ => {}
            }
        }

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
        let accessed = S3MetaData::as_unixtime(self.accessed);
        let created = S3MetaData::as_unixtime(self.created);
        let status_changed = S3MetaData::as_unixtime(self.status_changed);
        vec![
            ("modified".into(), modified),
            ("accessed".into(), accessed),
            ("created".into(), created),
            ("status_changed".into(), status_changed),
        ]
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

    fn accessed(&self) -> FsResult<SystemTime> {
        Ok(self.accessed)
    }

    fn created(&self) -> FsResult<SystemTime> {
        Ok(self.created)
    }

    fn status_changed(&self) -> FsResult<SystemTime> {
        Ok(self.status_changed)
    }

    fn executable(&self) -> FsResult<bool> {
        Ok(self.executable)
    }
}
