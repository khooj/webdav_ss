use clap::{App, Arg};
use webdav_ss::{application::Application, configuration::Configuration};

fn setup_tracing() {
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

#[tokio::main]
async fn main() {
    setup_tracing();

    let matches = App::new("webdav_ss")
        .version("0.1")
        .author("Igor Gilmutdinov <bladoff@gmail.com>")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("sets custom config file")
                .takes_value(true),
        )
        .get_matches();

    let config = matches.value_of("config").unwrap_or("webdav_ss.yml");

    let config = Configuration::new(config).expect("can't get configuration");
    let app = Application::build(config);
    app.run().await;
}
