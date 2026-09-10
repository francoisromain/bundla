use std::net::IpAddr;
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
use tokio::sync::broadcast;
use webadev::{Config, serve, watch};

use webadev_bundle::bundle;

#[derive(Parser)]
#[command(about = "bundle src into dist and serve it with live reload")]
struct Args {
    /// assets directory to watch and bundle
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// directory to serve
    #[arg(long, default_value = "dist")]
    dist: PathBuf,

    /// port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// ip address to bind to
    #[arg(short, long, default_value = "127.0.0.1")]
    ip: IpAddr,

    /// open the page in the browser on start
    #[arg(short, long)]
    open: bool,
}

#[tokio::main]
async fn main() {
    let Args {
        src,
        dist,
        port,
        ip,
        open,
    } = Args::parse();

    // src-change events: received when the user updates a file
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // reload events: sent after a successful bundle
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    if let Err(err) = watch(tx_src.clone(), &src) {
        eprintln!("Failed to watch {}: {err}", src.display());
        exit(1);
    }

    if let Err(err) = bundle(&src, &dist).await {
        eprintln!("bundle error: {err}");
        exit(1);
    }

    let src_clone = src.clone();
    let dist_clone = dist.clone();
    let tx_dist_clone = tx_dist.clone();
    tokio::spawn(async move {
        while let Ok((reload_type, paths)) = rx_src.recv().await {
            if let Err(err) = bundle(&src_clone, &dist_clone).await {
                eprintln!("bundle error: {err}");
                // keep the browser on the last good bundle
                continue;
            }
            let _ = tx_dist_clone.send((reload_type, paths));
        }
    });

    let config = Config {
        dir: dist,
        ip,
        port,
        headers: vec![],
        open,
    };

    if let Err(err) = serve(tx_dist, config).await {
        eprintln!("{err}");
        exit(1);
    }
}
