use testcontainers::{
    clients::Cli,
    images::generic::{GenericImage, Stream, WaitFor},
    Docker, Image, RunArgs,
};
use webdav_ss::backend::s3_backend::S3Backend;

#[tokio::test]
async fn test_s3_backend() {
    let img = RunArgs::default()
        .with_name("minio/minio")
        .with_mapped_port((9000, 9000))
        .with_mapped_port((9001, 9001));

    let img2 = GenericImage::new("minio/minio")
        .with_wait_for(WaitFor::LogMessage {
            message: "Detected default credentials".into(),
            stream: Stream::StdOut,
        })
        .with_args(vec!["server".into(), "/data".into()]);

    let docker = Cli::default();
    let cont = docker.run_with_args(img2, img);
    let port = cont.get_host_port(9001).unwrap();

    std::env::set_var("AWS_ACCESS_KEY_ID", "minioadmin");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "minioadmin");
    let backend = S3Backend::new(
        &format!("http://localhost:{}", port),
        "eu-central-1",
        "test_bucket",
    )
    .unwrap();
}
