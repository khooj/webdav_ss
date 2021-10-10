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
    pub props: HashMap<String, String>,
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
        let mut result = vec![("modified".into(), modified), ("created".into(), created)];
        for (k, v) in &self.props {
            result.push((k.clone(), v.clone()));
        }
        result
    }

    pub fn as_davprops(&self) -> Result<Vec<DavProp>, anyhow::Error> {
        let mut result = vec![];
        for (k, v) in &self.props {
            let k = base64::decode(k).unwrap_or(vec![]);
            if k.is_empty() {
                return Err(anyhow::anyhow!("cant decode prop name"));
            }
            let k = deserialize(&k[..]).unwrap_or(PropName::default());

            let p = base64::decode(v).unwrap_or(vec![]);
            if p.is_empty() {
                return Err(anyhow::anyhow!("cant decode prop"));
            }
            let p = deserialize(&p[..]).unwrap_or(Prop::default());
            result.push(DavProp {
                name: k.name,
                prefix: k.prefix,
                namespace: k.namespace,
                xml: p.value,
            });
        }
        Ok(result)
    }

    pub fn save_davprop(&mut self, prop: DavProp) -> Result<(), anyhow::Error> {
        let k = PropName {
            name: prop.name,
            prefix: prop.prefix,
            namespace: prop.namespace,
        };
        let k = serialize(&k).unwrap_or(vec![]);
        if k.is_empty() {
            return Err(anyhow::anyhow!("cant serialize name"));
        }
        let k = base64::encode(&k[..]);
        if k.chars().count() > 128 {
            return Err(anyhow::anyhow!(
                "dav prop name length cannot exceed 128 Unicode characters"
            ));
        }

        let p = self.props.entry(k).or_insert("".into());
        let prop = Prop { value: prop.xml };
        let result = serialize(&prop)?;
        let result = base64::encode(result);
        // i think i should check length of grapheme cluster but its ok for now.
        if result.chars().count() > 256 {
            return Err(anyhow::anyhow!(
                "dav prop length cannot exceed 256 Unicode characters"
            ));
        }
        *p = result;
        Ok(())
    }

    pub fn remove_davprop(&mut self, prop: DavProp) {
        let _ = self.props.remove_entry(&prop.name);
    }

    pub fn get_prop(&self, prop: DavProp) -> Option<Vec<u8>> {
        let name = PropName{
            name: prop.name,
            prefix: prop.prefix,
            namespace: prop.namespace,
        };
        let k = serialize(&name).unwrap_or(vec![]);
        if k.is_empty() {
            return None;
        }
        let k = base64::encode(&k);
        let result = self.props.get(&k);
        if result.is_none() {
            return None;
        }

        let result = result.unwrap();
        let result = base64::decode(result).unwrap_or(vec![]);
        if result.is_empty() {
            return None;
        }
        let result = deserialize(&result[..]).unwrap_or(Prop::default());
        result.value
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
