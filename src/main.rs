use std::net::IpAddr;
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;

use bundla::{BundlerOptions, Server, ServerConfig, bundle, dev, serve};

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

enum Mode {
    Dev,
    Release,
    Serve,
    ReleaseThenServe,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(err) = run(&args).await {
        eprintln!("{err}");
        exit(1);
    }
}

async fn run(args: &Args) -> Result<(), String> {
    let dist = output_dir(args);

    if args.open && args.release && !args.serve {
        return Err("error: --open requires a running server (--serve)".to_string());
    }
    if args.release && !args.serve && !args.headers.is_empty() {
        return Err("error: --header requires a running server (--serve)".to_string());
    }

    match mode_select(args.release, args.serve) {
        Mode::Dev => {
            let server = dev(&args.src, &server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
        Mode::Release => {
            bundle(&args.src, &dist, BundlerOptions::RELEASE).await?;
            println!("bundled {}", dist.display());
            Ok(())
        }
        Mode::Serve => {
            let server = serve(&server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
        Mode::ReleaseThenServe => {
            bundle(&args.src, &dist, BundlerOptions::RELEASE).await?;
            println!("bundled {}", dist.display());
            let server = serve(&server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
    }
}

fn server_config_build(args: &Args, dist: PathBuf) -> ServerConfig {
    ServerConfig {
        dir: dist,
        ip: args.ip,
        port: args.port,
        headers: args.headers.clone(),
    }
}

fn mode_select(release: bool, serve: bool) -> Mode {
    match (release, serve) {
        (false, false) => Mode::Dev,
        (true, false) => Mode::Release,
        (false, true) => Mode::Serve,
        (true, true) => Mode::ReleaseThenServe,
    }
}

fn output_dir(args: &Args) -> PathBuf {
    if let Some(dist) = &args.dist {
        return dist.clone();
    }

    if args.release || args.serve {
        PathBuf::from("dist")
    } else {
        PathBuf::from("dev")
    }
}

async fn start(server: Server, open: bool) -> Result<(), String> {
    println!("Serving {}", server.url);

    if open && let Err(err) = open::that(&server.url) {
        eprintln!("Failed to open browser: {err}");
    }

    server.run().await
}
