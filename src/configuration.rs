use std::ops::Deref;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

pub fn setup_tracing() {
    use tracing_subscriber::{fmt, prelude::*, registry::Registry, EnvFilter};

    let fmt_subscriber = fmt::layer();

    let env_subscriber = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let collector = Registry::default()
        .with(fmt_subscriber)
        .with(env_subscriber);

    tracing_log::LogTracer::init().expect("can't set log tracer");
    tracing::subscriber::set_global_default(collector).expect("can't set global default");
}

#[derive(Debug, Deserialize)]
pub struct Application {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Filesystem {
    FS {
        path: String,
    },
    Mem,
    S3 {
        bucket: String,
        region: String,
        url: String,
        path_style: bool,
        ensure_bucket: bool,
        #[serde(default)]
        auth: S3Authentication,
    },
}

#[derive(Debug, Deserialize, Clone, derivative::Derivative)]
#[derivative(Default)]
pub struct ConfAccessKey(#[derivative(Default(value = "\"AWS_ACCESS_KEY_ID\".into()"))] String);

impl Deref for ConfAccessKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Deserialize, Clone, derivative::Derivative)]
#[derivative(Default)]
pub struct ConfSecretKey(#[derivative(Default(value = "\"AWS_SECRET_ACCESS_KEY\".into()"))] String);

impl Deref for ConfSecretKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize)]
pub struct S3AuthFile {
    #[serde(rename = "ACCESS_KEY")]
    pub access_key: String,
    #[serde(rename = "SECRET_KEY")]
    pub secret_key: String,
}

#[derive(Debug, Deserialize, Clone, derivative::Derivative)]
#[serde(tag = "type", rename_all = "lowercase")]
#[derivative(Default)]
pub enum S3Authentication {
    #[derivative(Default)]
    Environment {
        #[serde(default)]
        access_key: ConfAccessKey,
        #[serde(default)]
        secret_key: ConfSecretKey,
    },
    File {
        path: String,
    },
    Values {
        access_key_value: String,
        secret_key_value: String,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilesystemType {
    #[serde(flatten)]
    pub fs: Filesystem,
    pub mount_path: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PropsStorage {
    Yaml { path: String },
    Mem,
}

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub app: Application,
    pub filesystems: Vec<FilesystemType>,
    pub prop_storage: Option<PropsStorage>,
}

impl Configuration {
    pub fn new(filename: &str) -> Result<Self, ConfigError> {
        let mut s = Config::default();
        s.merge(File::with_name(filename))?;
        s.merge(Environment::with_prefix("app"))?;
        s.try_into()
    }
}
