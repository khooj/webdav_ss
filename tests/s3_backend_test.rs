use futures_util::{future::FusedFuture, join, select, FutureExt};
use std::env;
use testcontainers::{
    clients::Cli,
    images::generic::{GenericImage, Stream, WaitFor},
    Docker, Image, RunArgs,
};
use tokio::{process::*, runtime::Runtime};
use webdav_ss::{
    application::Application,
    backend::s3_backend::S3Backend,
    configuration::{Application as ConfigApplication, Configuration, Filesystem, FilesystemType},
};

#[tokio::test]
async fn test_s3_backend() {
    let args = RunArgs::default()
        .with_mapped_port((9000, 9000))
        .with_mapped_port((9001, 9001));

    let image = GenericImage::new("minio/minio")
        .with_wait_for(WaitFor::LogMessage {
            message: "Detected default credentials".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["server".into(), "/data".into()]);

    env::set_var("AWS_ACCESS_KEY_ID", "minioadmin");
    env::set_var("AWS_SECRET_ACCESS_KEY", "minioadmin");

    let docker = Cli::default();
    let cont = docker.run_with_args(image, args);
    let port = cont.get_host_port(9001).unwrap();

    let config = Configuration {
        app: ConfigApplication {
            host: "127.0.0.1".into(),
            port: 8080,
        },
        filesystems: vec![Filesystem {
            mount_path: "/".into(),
            path: None,
            bucket: Some("test".into()),
            typ: FilesystemType::S3,
            url: Some(format!("http://localhost:{}", port)),
        }],
    };

    let mut app = Box::pin(Application::build(config).run().fuse());
    let cmd = Command::new("litmus")
        .arg(format!("http://localhost:8080"))
        .env("TESTS", "basic")
        .current_dir(env::current_dir().unwrap())
        .output()
        .fuse();
    let mut cmd = Box::pin(cmd);

    select! {
        res = cmd => {
            let result = res.unwrap();
            let stdout = String::from_utf8(result.stdout).unwrap();
            let stderr = String::from_utf8(result.stderr).unwrap();
            cont.stop();
            cont.rm();

            println!("stdout: {}", stdout);
            println!("stderr: {}", stderr);
            assert!(result.status.success());
        },
        _ = app => {} ,
    };
}
