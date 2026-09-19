use std::path::{Path, PathBuf};

use tokio::sync::broadcast::{self, Sender};
use webadev::{Config, ReloadType, Server, watch};

use crate::{BundlerOptions, bundle};

/// a runtime log line from the dev server, sent on the caller's broadcast
/// channel so the library never prints directly.
#[derive(Clone, Debug)]
pub enum DevLog {
    /// a source change detected by the watcher, before the rebundle
    Change(ReloadType, Vec<PathBuf>),
    /// a non-fatal bundle warning (unreadable asset skipped, etc.)
    Warn(String),
    /// a rebundle failure during watch; the browser keeps the previous bundle
    Error(String),
}

/// serve `config.dir` without watching or bundling.
pub async fn serve(config: &Config) -> Result<Server, String> {
    Server::new(config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))
}

/// watch `src`, bundle it into `config.dir` on startup and on every change,
/// and serve it with live reload. Runtime logs are reported on `log_tx`.
pub async fn dev(src: &Path, config: &Config, tx_log: Sender<DevLog>) -> Result<Server, String> {
    // src-change events: when the user updates files in the src directory
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // dist-change: when a bundle updates files in the dis directory
    // triggers a browser reload
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    watch(tx_src.clone(), src)
        .map_err(|err| format!("failed to watch {}: {err}", src.display()))?;

    // bundle once on startup
    bundle(src, &config.dir, BundlerOptions::DEV)
        .await
        .map_err(|err| format!("bundle error: {err}"))?
        .into_iter()
        .for_each(|warning| {
            let _ = tx_log.send(DevLog::Warn(warning));
        });

    let server = Server::bind(tx_dist.clone(), config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))?;

    let src_clone = src.to_path_buf();
    let dist_clone = config.dir.clone();
    let tx_dist_clone = tx_dist.clone();
    tokio::spawn(async move {
        while let Ok((reload_type, paths)) = rx_src.recv().await {
            let _ = tx_log.send(DevLog::Change(reload_type, paths.clone()));
            match bundle(&src_clone, &dist_clone, BundlerOptions::DEV).await {
                Ok(warnings) => {
                    warnings.into_iter().for_each(|warning| {
                        let _ = tx_log.send(DevLog::Warn(warning));
                    });
                    let _ = tx_dist_clone.send((reload_type, paths));
                }
                Err(err) => {
                    let _ = tx_log.send(DevLog::Error(format!("bundle error: {err}")));
                    // keep the browser on the previous bundle
                }
            }
        }
    });

    Ok(server)
}
