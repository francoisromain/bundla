use std::net::IpAddr;
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;

use webadle::{Config, run};

#[derive(Parser)]
#[command(about = "bundle src into dist; dev server by default, --release builds artifacts only")]
struct Args {
    /// assets directory to watch and bundle
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// output or serving directory (default: dev/, or dist/ with --release or --serve)
    #[arg(long)]
    dist: Option<PathBuf>,

    /// release build: minify, hash filenames, then exit (or serve with --serve)
    #[arg(short, long)]
    release: bool,

    /// serve an output directory without rebundling (after --release: serve the fresh build)
    #[arg(long)]
    serve: bool,

    /// port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// ip address to bind to
    #[arg(short, long, default_value = "127.0.0.1")]
    ip: IpAddr,

    /// open the page in the browser on start
    #[arg(short, long)]
    open: bool,

    /// additional HTTP header on every response (repeatable)
    #[arg(long = "header", value_name = "NAME: VALUE")]
    headers: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let config = Config {
        src: args.src,
        dist: args.dist,
        release: args.release,
        serve: args.serve,
        port: args.port,
        ip: args.ip,
        open: args.open,
        headers: args.headers,
    };

    if let Err(err) = run(&config).await {
        eprintln!("{err}");
        exit(1);
    }
}
