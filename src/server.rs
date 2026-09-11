use std::path::Path;

use tokio::sync::broadcast;
use webadev::{Config, Server, watch};

use crate::{BundlerOptions, bundle};

/// serve `config.dir` without watching or bundling.
pub async fn serve(config: &Config) -> Result<Server, String> {
    Server::new(config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))
}

/// watch `src`, bundle it into `config.dir` on startup and on every change,
/// and serve it with live reload.
pub async fn serve_dev(src: &Path, config: &Config) -> Result<Server, String> {
    // src-change events: received when the user updates a file
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // reload events: sent after a successful bundle
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    watch(tx_src.clone(), src)
        .map_err(|err| format!("failed to watch {}: {err}", src.display()))?;

    bundle(src, &config.dir, BundlerOptions::DEV)
        .await
        .map_err(|err| format!("bundle error: {err}"))?;

    let server = Server::bind(tx_dist.clone(), config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))?;

    let src_clone = src.to_path_buf();
    let dist_clone = config.dir.clone();
    let tx_dist_clone = tx_dist.clone();
    tokio::spawn(async move {
        while let Ok((reload_type, paths)) = rx_src.recv().await {
            if let Err(err) = bundle(&src_clone, &dist_clone, BundlerOptions::DEV).await {
                eprintln!("bundle error: {err}");
                // keep the browser on the last good bundle
                continue;
            }
            let _ = tx_dist_clone.send((reload_type, paths));
        }
    });

    Ok(server)
}
