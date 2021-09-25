use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Application {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum FilesystemType {
    FS,
    Mem,
}

#[derive(Debug, Deserialize)]
pub struct Filesystem {
    pub path: Option<String>,
    pub mount_path: String,
    #[serde(rename = "type")]
    pub typ: FilesystemType,
}

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub app: Application,
    pub filesystems: Vec<Filesystem>,
}

impl Configuration {
    pub fn new(filename: &str) -> Result<Self, ConfigError> {
        let mut s = Config::default();
        s.merge(File::with_name(filename))?;
        s.merge(Environment::with_prefix("app"))?;
        s.try_into()
    }
}