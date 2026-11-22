use webadev::{Config, Server};

/// Serve `config.dir` without watching or building.
/// Optional `config.base` prefix.
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
///         base: String::new(),
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
