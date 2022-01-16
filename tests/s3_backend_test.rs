use futures_util::{select, FutureExt};
use std::env;
use testcontainers::{
    clients::Cli,
    images::generic::{GenericImage, Stream, WaitFor},
    Container, Docker, Image, RunArgs,
};
use tokio::process::*;
use webdav_ss::{
    application::Application,
    configuration::{
        Application as ConfigApplication, Configuration, Filesystem, FilesystemType, PropsStorage,
        S3Authentication,
    },
};

struct ContainerDrop<'d, D: Docker, I: Image> {
    container: Container<'d, D, I>,
}

impl<'d, D: Docker, I: Image> Drop for ContainerDrop<'d, D, I> {
    fn drop(&mut self) {
        self.container.stop();

        let mut container_stdout = String::new();
        self.container
            .logs()
            .stdout
            .read_to_string(&mut container_stdout)
            .unwrap();
        let mut container_stderr = String::new();
        self.container
            .logs()
            .stderr
            .read_to_string(&mut container_stderr)
            .unwrap();

        println!("container stdout: {}", container_stdout);
        println!("container stderr: {}", container_stderr);

        self.container.rm();
    }
}

async fn run_in_container(image: GenericImage, args: RunArgs, fs: FilesystemType) {
    let docker = Cli::default();
    let cont = docker.run_with_args(image, args);

    let _cont = ContainerDrop { container: cont };

    let config = Configuration {
        app: ConfigApplication {
            host: "127.0.0.1".into(),
            port: 8080,
        },
        filesystems: vec![
            fs,
            FilesystemType {
                mount_path: "/fs2".into(),
                fs: Filesystem::Mem,
            },
        ],
        prop_storage: Some(PropsStorage::Yaml {
            path: "/tmp/webdav_props.yml".into(),
        }),
    };

    if std::fs::metadata("/tmp/webdav_props.yml")
        .map(|_| true)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file("/tmp/webdav_props.yml");
    }

    let mut app = Box::pin(Application::build(config).await.run().fuse());
    let cmd = Command::new("litmus")
        .arg(format!("http://localhost:8080/fs3"))
        .current_dir(env::current_dir().unwrap())
        .output()
        .fuse();
    let mut cmd = Box::pin(cmd);

    select! {
        res = cmd => {
            let result = res.unwrap();
            let stdout = String::from_utf8(result.stdout).unwrap();
            let stderr = String::from_utf8(result.stderr).unwrap();

            println!("stdout: {}", stdout);
            println!("stderr: {}", stderr);
            assert!(result.status.success());
        },
        _ = app => {} ,
    };
}

#[tokio::test]
// #[cfg(feature = "integration")]
async fn s3_backend_minio() {
    // env::set_var("RUST_LOG", "webdav_ss=debug,webdav_handler=debug");
    webdav_ss::configuration::setup_tracing();

    let args = RunArgs::default().with_mapped_port((9000, 9000));
    let image = GenericImage::new("minio/minio")
        .with_wait_for(WaitFor::LogMessage {
            message: "Detected default credentials".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["server".into(), "/data".into()])
        .with_env_var("MINIO_DOMAIN", "localhost");

    env::set_var("AWS_ACCESS_KEY_ID", "minioadmin");
    env::set_var("AWS_SECRET_ACCESS_KEY", "minioadmin");

    let fs = FilesystemType {
        mount_path: "/fs3".into(),
        fs: Filesystem::S3 {
            region: "us-east-1".into(),
            bucket: "test".into(),
            url: format!("http://localhost:{}", 9000),
            path_style: false,
            ensure_bucket: true,
            auth: S3Authentication::Values {
                access_key: "minioadmin".into(),
                secret_key: "minioadmin".into(),
            },
        },
    };

    run_in_container(image, args, fs).await;
}

#[tokio::test]
// #[cfg(feature = "integration")]
async fn s3_backend_minio_pathstyle() {
    // env::set_var("RUST_LOG", "webdav_ss=debug,webdav_handler=debug");
    webdav_ss::configuration::setup_tracing();

    let args = RunArgs::default().with_mapped_port((9000, 9000));
    let image = GenericImage::new("minio/minio")
        .with_wait_for(WaitFor::LogMessage {
            message: "Detected default credentials".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["server".into(), "/data".into()]);

    let fs = FilesystemType {
        mount_path: "/fs3".into(),
        fs: Filesystem::S3 {
            region: "us-east-1".into(),
            bucket: "test".into(),
            url: format!("http://localhost:{}", 9000),
            path_style: true,
            ensure_bucket: true,
            auth: S3Authentication::Values {
                access_key: "minioadmin".into(),
                secret_key: "minioadmin".into(),
            },
        },
    };

    run_in_container(image, args, fs).await;
}

#[tokio::test]
// #[cfg(feature = "integration")]
async fn s3_backend_zenko() {
    env::set_var("RUST_LOG", "webdav_ss=debug,webdav_handler=debug");
    webdav_ss::configuration::setup_tracing();

    let args = RunArgs::default().with_mapped_port((8000, 8000));
    let image = GenericImage::new("zenko/cloudserver")
        .with_wait_for(WaitFor::LogMessage {
            message: "server started".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["yarn".into(), "run".into(), "mem_backend".into()]);

    let fs = FilesystemType {
        mount_path: "/fs3".into(),
        fs: Filesystem::S3 {
            region: "us-east-1".into(),
            bucket: "test".into(),
            url: format!("http://localhost:{}", 8000),
            path_style: true,
            ensure_bucket: true,
            auth: S3Authentication::Values {
                access_key: "accessKey1".into(),
                secret_key: "verySecretKey1".into(),
            },
        },
    };

    run_in_container(image, args, fs).await;
}
