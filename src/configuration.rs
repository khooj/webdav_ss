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
    pub cert: Option<String>,
    pub key: Option<String>,
    pub tls: bool,
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
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilesystemType {
    #[serde(flatten)]
    pub fs: Filesystem,
    pub mount_path: String,
}

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub app: Application,
    pub filesystems: Vec<FilesystemType>,
}

impl Configuration {
    pub fn new(filename: &str) -> Result<Self, ConfigError> {
        let mut s = Config::default();
        s.merge(File::with_name(filename))?;
        s.merge(Environment::with_prefix("app"))?;
        s.try_into()
    }
}
