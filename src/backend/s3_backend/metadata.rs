use s3::serde_types::Tagging;
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
    pub props: HashMap<String, String>,
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
                &"created" => metadata.created = S3MetaData::extract_unixtime_or_zero(&v),
                prop => {
                    let _ = metadata.props.entry(prop.to_string()).or_insert(v);
                }
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
        let created = S3MetaData::as_unixtime(self.created);
        let mut result = vec![
            ("modified".into(), modified),
            ("created".into(), created),
        ];
        for (k, v) in &self.props {
            result.push((k.clone(), v.clone()));
        }
        result
    }

    pub fn as_davprops(&self) -> Vec<DavProp> {
        let mut result = vec![];
        for (k, v) in &self.props {
            result.push(DavProp {
                name: k.clone(),
                namespace: None,
                prefix: None,
                xml: Some(v.clone().into()),
            });
        }
        result
    }

    pub fn save_davprop(&mut self, prop: DavProp) {
        let p = self.props.entry(prop.name).or_insert("".into());
        if let Some(v) = prop.xml {
            *p = String::from_utf8_lossy(&v[..]).to_string();
        }
    }

    pub fn remove_davprop(&mut self, prop: DavProp) {
        let _ = self.props.remove_entry(&prop.name);
    }

    pub fn get_prop(&self, prop: DavProp) -> Option<Vec<u8>> {
        let result = self.props.get(&prop.name);
        result.map(|f| f.clone().into_bytes())
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
