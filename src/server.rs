use std::path::Path;

use tokio::sync::broadcast::{self, Sender};
use webadev::{Config, Server};

use crate::{
    BundlerOptions,
    watch::{WatchLog, watch_setup},
};

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

/// Watch `src`,
/// bundle it into `config.dir` on startup and on every change,
/// serve it with live reload.
/// `options` selects the build profile
/// (`BundlerOptions::DEV` for the usual dev loop).
/// Runtime logs are reported on `tx_log`.
///
/// This is the headless [`watch`](crate::watch) with a server on
/// top: on every successful rebundle the connected browsers reload
/// (hot-reload for css when filenames are stable, full page reload
/// otherwise — hashed filenames change on rebuild), and a failed
/// rebundle keeps the previous bundle. The reload script is served,
/// never written to the output files.
///
/// # Examples
///
/// ```
/// use std::net::IpAddr;
/// use std::path::Path;
///
/// use bundla::{BundlerOptions, ServerConfig, watch_serve};
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
///     let server = watch_serve(Path::new(&src), &config, BundlerOptions::DEV, tx_log)
///         .await
///         .unwrap();
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
pub async fn watch_serve(
    src: &Path,
    config: &Config,
    options: BundlerOptions,
    tx_log: Sender<WatchLog>,
) -> Result<Server, String> {
    // Dist-change events: when a bundle updates files in the dist directory
    // triggers a browser reload
    let (tx_dist, _rx_dist) = broadcast::channel(100);

    // Watch `src`, bundle once on startup
    let watcher = watch_setup(src, &config.dir, options, tx_log, Some(tx_dist.clone())).await?;

    let server = Server::bind(tx_dist.clone(), config.clone())
        .await
        .map_err(|err| format!("bind error: {err}"))?;

    // Rebundle on every change, then reload the browser
    tokio::spawn(watcher.run());

    Ok(server)
}
