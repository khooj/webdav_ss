use futures_util::{future::FusedFuture, join, select, FutureExt};
use std::env;
use testcontainers::{
    clients::Cli,
    images::generic::{GenericImage, Stream, WaitFor},
    Container, Docker, Image, RunArgs,
};
use tokio::{process::*, runtime::Runtime};
use webdav_ss::{
    application::Application,
    backend::s3_backend::S3Backend,
    configuration::{Application as ConfigApplication, Configuration, Filesystem, FilesystemType},
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

#[tokio::test]
async fn test_s3_backend() {
    env::set_var("RUST_LOG", "debug");
    webdav_ss::configuration::setup_tracing();

    let args = RunArgs::default().with_mapped_port((9000, 9000));

    let image = GenericImage::new("minio/minio")
        .with_wait_for(WaitFor::LogMessage {
            message: "Detected default credentials".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["server".into(), "/data".into()])
        .with_env_var("MINIO_DOMAIN", "localhost");


    let docker = Cli::default();
    let cont = docker.run_with_args(image, args);
    let port = cont.get_host_port(9000).unwrap();

    let _cont = ContainerDrop { container: cont };

    let config = Configuration {
        app: ConfigApplication {
            host: "127.0.0.1".into(),
            port: 8080,
        },
        filesystems: vec![Filesystem {
            mount_path: "/".into(),
            region: Some("us-east-1".into()),
            path: None,
            bucket: Some("test".into()),
            typ: FilesystemType::S3,
            url: Some(format!("http://localhost:{}", port)),
        }],
    };

    env::set_var("AWS_ACCESS_KEY_ID", "minioadmin");
    env::set_var("AWS_SECRET_ACCESS_KEY", "minioadmin");
    let mut app = Box::pin(Application::build(config).await.run().fuse());
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

            println!("stdout: {}", stdout);
            println!("stderr: {}", stderr);
            assert!(result.status.success());
        },
        _ = app => {} ,
    };
}
