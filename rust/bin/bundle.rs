use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::Parser;
use tokio::sync::broadcast;
use webadev::{Config, serve, watch};

use webadev_bundle::{Options, bundle};

#[derive(Parser)]
#[command(about = "bundle src into dist; dev server by default, --release builds artifacts only")]
struct Args {
    /// assets directory to watch and bundle
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// output or serving directory (default: dev/, or dist/ with --release or --serve)
    #[arg(long)]
    dist: Option<PathBuf>,

    /// production build: minify, hash filenames, then exit (or serve with --serve)
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

enum RunMode {
    Dev,
    Build,
    Serve,
    BuildThenServe,
}

fn run_mode(release: bool, serve: bool) -> RunMode {
    match (release, serve) {
        (false, false) => RunMode::Dev,
        (true, false) => RunMode::Build,
        (false, true) => RunMode::Serve,
        (true, true) => RunMode::BuildThenServe,
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let dist = output_dir(&args);

    if args.open && args.release && !args.serve {
        eprintln!("error: --open requires a running server (--serve)");
        exit(1);
    }
    if args.release && !args.serve && !args.headers.is_empty() {
        eprintln!("warning: --header has no effect without --serve");
    }

    match run_mode(args.release, args.serve) {
        RunMode::Dev => dev(&args, dist).await,
        RunMode::Build => release_bundle(&args.src, &dist).await,
        RunMode::Serve => serve_existing(&args, dist).await,
        RunMode::BuildThenServe => {
            release_bundle(&args.src, &dist).await;
            serve_existing(&args, dist).await;
        }
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

fn config(args: &Args, dir: PathBuf) -> Config {
    Config {
        dir,
        ip: args.ip,
        port: args.port,
        headers: args.headers.clone(),
        open: args.open,
    }
}

async fn release_bundle(src: &Path, dist: &Path) {
    match bundle(src, dist, Options::RELEASE).await {
        Ok(()) => println!("bundled {}", dist.display()),
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    }
}

async fn serve_existing(args: &Args, dist: PathBuf) {
    let (tx_dist, _rx) = broadcast::channel(100);
    if let Err(err) = serve(tx_dist, config(args, dist)).await {
        eprintln!("{err}");
        exit(1);
    }
}

async fn dev(args: &Args, dist: PathBuf) {
    let src = &args.src;

    // src-change events: received when the user updates a file
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // reload events: sent after a successful bundle
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    if let Err(err) = watch(tx_src.clone(), src) {
        eprintln!("Failed to watch {}: {err}", src.display());
        exit(1);
    }

    if let Err(err) = bundle(src, &dist, Options::DEV).await {
        eprintln!("bundle error: {err}");
        exit(1);
    }

    let src_clone = src.clone();
    let dist_clone = dist.clone();
    let tx_dist_clone = tx_dist.clone();
    tokio::spawn(async move {
        while let Ok((reload_type, paths)) = rx_src.recv().await {
            if let Err(err) = bundle(&src_clone, &dist_clone, Options::DEV).await {
                eprintln!("bundle error: {err}");
                // keep the browser on the last good bundle
                continue;
            }
            let _ = tx_dist_clone.send((reload_type, paths));
        }
    });

    if let Err(err) = serve(tx_dist, config(args, dist)).await {
        eprintln!("{err}");
        exit(1);
    }
}
