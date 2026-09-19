use std::path::{Path, PathBuf};

use tokio::sync::broadcast::{self, Sender};
use webadev::{Config, ReloadType, Server, watch};

use crate::{BundlerOptions, bundle};

/// A runtime log line from the dev server, sent on the caller's broadcast
/// channel so the library never prints directly.
#[derive(Clone, Debug)]
pub enum DevLog {
    /// A source change detected by the watcher, before the rebundle.
    Change(ReloadType, Vec<PathBuf>),
    /// A non-fatal bundle warning (unreadable asset skipped, etc.).
    Warn(String),
    /// A rebundle failure during watch; the browser keeps the previous bundle.
    Error(String),
}

/// Serve `config.dir` without watching or bundling.
///
/// # Examples
///
/// ```
/// use std::net::IpAddr;
///
/// use bundla::{ServerConfig, serve};
/// use tempfile::tempdir;
///
/// #[tokio::main]
/// async fn main() {
///     let dir = tempdir().unwrap();
///     std::fs::write(dir.path().join("index.html"), "hello from serve").unwrap();
///
///     let config = ServerConfig {
///         dir: dir.path().into(),
///         ip: IpAddr::from([127, 0, 0, 1]),
///         port: 0,
///         headers: vec![],
///         index: "index.html".into(),
///     };
///     let server = serve(&config).await.unwrap();
///     let url = server.url.clone();
///     let handle = tokio::spawn(async move { server.run().await });
///
///     let body = reqwest::get(&url).await.unwrap().text().await.unwrap();
///     assert!(body.contains("hello from serve"));
///
///     handle.abort();
/// }
/// ```
pub async fn serve(config: &Config) -> Result<Server, String> {
    Server::new(config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))
}

/// Watch `src`, bundle it into `config.dir` on startup and on every change,
/// and serve it with live reload. Runtime logs are reported on `tx_log`.
///
/// # Examples
///
/// ```
/// use std::net::IpAddr;
/// use std::path::Path;
///
/// use bundla::{ServerConfig, dev};
/// use tempfile::tempdir;
///
/// #[tokio::main]
/// async fn main() {
///     let dir = tempdir().unwrap();
///     let src = dir.path().join("src");
///     std::fs::create_dir_all(src.join("styles")).unwrap();
///     std::fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
///     std::fs::write(
///         src.join("index.html"),
///         "<html><head><link rel=\"stylesheet\" href=\"./styles/main.css\"></head>\
///          <body><h1>hi</h1></body></html>",
///     )
///     .unwrap();
///
///     let config = ServerConfig {
///         dir: dir.path().join("dev"),
///         ip: IpAddr::from([127, 0, 0, 1]),
///         port: 0,
///         headers: vec![],
///         index: "index.html".into(),
///     };
///
///     let (tx_log, _rx_log) = tokio::sync::broadcast::channel(100);
///     let server = dev(Path::new(&src), &config, tx_log).await.unwrap();
///     let url = server.url.clone();
///     let handle = tokio::spawn(async move { server.run().await });
///
///     let body = reqwest::get(&url).await.unwrap().text().await.unwrap();
///     assert!(body.contains("<h1>hi</h1>"));
///     assert!(body.contains("main.css"));
///
///     handle.abort();
/// }
/// ```
pub async fn dev(src: &Path, config: &Config, tx_log: Sender<DevLog>) -> Result<Server, String> {
    // Src-change events: when the user updates files in the src directory
    let (tx_src, mut rx_src) = broadcast::channel(100);
    // Dist-change events: when a bundle updates files in the dist directory
    // triggers a browser reload
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    watch(tx_src.clone(), src)
        .map_err(|err| format!("failed to watch {}: {err}", src.display()))?;

    // Bundle once on startup
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
                    // Keep the browser on the previous bundle
                }
            }
        }
    });

    Ok(server)
}
