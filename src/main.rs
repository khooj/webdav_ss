use clap::{App, Arg};
use webdav_ss::{
    application::Application,
    configuration::{setup_tracing, Configuration},
};

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
