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

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum FilesystemType {
    FS,
    Mem,
    S3,
}

#[derive(Debug, Deserialize)]
pub struct Filesystem {
    pub path: Option<String>,
    pub mount_path: String,
    #[serde(rename = "type")]
    pub typ: FilesystemType,
    pub url: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
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