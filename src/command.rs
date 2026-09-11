use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};

use tokio::sync::broadcast;
use webadev::{
    Config as WebadevConfig, ReloadType, bind as webadev_bind, serve as webadev_serve,
    watch as webadev_watch,
};

use crate::{Options, bundle};

enum Mode {
    Dev,
    Build,
    Serve,
    BuildThenServe,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub src: PathBuf,
    pub dist: Option<PathBuf>,
    pub release: bool,
    pub serve: bool,
    pub port: u16,
    pub ip: IpAddr,
    pub open: bool,
    pub headers: Vec<String>,
}

pub async fn run(config: &Config) -> Result<(), String> {
    let dist = output_dir(config);

    if config.open && config.release && !config.serve {
        return Err("error: --open requires a running server (--serve)".to_string());
    }
    if config.release && !config.serve && !config.headers.is_empty() {
        return Err("error: --header requires a running server (--serve)".to_string());
    }

    match mode_select(config.release, config.serve) {
        Mode::Dev => dev(config, dist).await,
        Mode::Build => release(&config.src, &dist).await,
        Mode::Serve => dir_serve(config, dist).await,
        Mode::BuildThenServe => {
            release(&config.src, &dist).await?;
            dir_serve(config, dist).await
        }
    }
}

fn mode_select(release: bool, serve: bool) -> Mode {
    match (release, serve) {
        (false, false) => Mode::Dev,
        (true, false) => Mode::Build,
        (false, true) => Mode::Serve,
        (true, true) => Mode::BuildThenServe,
    }
}

fn output_dir(config: &Config) -> PathBuf {
    if let Some(dist) = &config.dist {
        return dist.clone();
    }

    if config.release || config.serve {
        PathBuf::from("dist")
    } else {
        PathBuf::from("dev")
    }
}

async fn release(src: &Path, dist: &Path) -> Result<(), String> {
    bundle(src, dist, Options::RELEASE).await?;
    println!("bundled {}", dist.display());
    Ok(())
}

async fn dir_serve(config: &Config, dist: PathBuf) -> Result<(), String> {
    let (tx_dist, _rx) = broadcast::channel(100);
    serve(tx_dist, config, dist).await
}

async fn dev(config: &Config, dist: PathBuf) -> Result<(), String> {
    let src = &config.src;

    // src-change events: received when the user updates a file
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // reload events: sent after a successful bundle
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    webadev_watch(tx_src.clone(), src)
        .map_err(|err| format!("failed to watch {}: {err}", src.display()))?;

    bundle(src, &dist, Options::DEV)
        .await
        .map_err(|err| format!("bundle error: {err}"))?;

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

    serve(tx_dist, config, dist).await
}

async fn serve(
    tx: broadcast::Sender<(ReloadType, Vec<PathBuf>)>,
    config: &Config,
    dist: PathBuf,
) -> Result<(), String> {
    let (url, listener, router) = webadev_bind(tx, webadev_config_build(config, dist))
        .await
        .map_err(|err| format!("bind error: {err}"))?;

    println!("Serving {url}");

    if config.open
        && let Err(err) = open::that(&url)
    {
        eprintln!("Failed to open browser: {err}");
    }

    webadev_serve(listener, router).await.map(|_| ())
}

fn webadev_config_build(config: &Config, dir: PathBuf) -> WebadevConfig {
    WebadevConfig {
        dir,
        ip: config.ip,
        port: config.port,
        headers: config.headers.clone(),
    }
}
