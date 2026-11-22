#![cfg(all(feature = "css", feature = "watch", feature = "serve"))]

use std::{fs, net::IpAddr, time::Duration};

use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::broadcast,
    time::timeout,
};

use bundla::{BuildOptions, ServerConfig, WatchLog, watch_serve};

// A hashed profile (`-sw`) must reload the full page on css edits: css
// hot-reload re-fetches the stylesheet href held by the page, and the
// build serves a new hashed name — the stale href would 404. The
// wire format is `event: <name>` on the SSE stream.
#[tokio::test]
async fn watch_serve_hashed_css_change_reloads_full_page() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        "<html><head><link rel=\"stylesheet\" href=\"./styles/main.css\"></head>\
         <body>v1</body></html>",
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();

    let config = ServerConfig {
        dir: dir.path().join("dist"),
        ip: IpAddr::from([127, 0, 0, 1]),
        port: 0,
        headers: vec![],
        index: "index.html".into(),
        base: String::new(),
    };

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let server = watch_serve(&src, &config, &BuildOptions::PROD, tx_log, &[])
        .await
        .unwrap();

    // the css is hashed in the initial bundle
    let hashed_css = fs::read_dir(config.dir.join("styles"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .find(|name| name.ends_with(".css"))
        .expect("a hashed css output");
    assert!(hashed_css.starts_with("main-"), "got: {hashed_css}");

    let url = server.url.clone();
    let handle = tokio::spawn(async move { server.run().await });

    // subscribe with a raw TCP connection: the SSE stream never ends, so
    // `reqwest` would wait forever on it
    let mut stream = TcpStream::connect(url.trim_start_matches("http://"))
        .await
        .unwrap();
    stream
        .write_all(b"GET /livereload HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    // read the headers first: once they arrive the subscription is live
    // and no broadcast event can be missed (there is no replay)
    let mut text = String::new();
    let mut buf = [0; 4096];
    let headers = timeout(Duration::from_secs(10), async {
        loop {
            let read = stream.read(&mut buf).await.unwrap();
            assert!(read > 0, "stream closed before the headers");
            text.push_str(&String::from_utf8_lossy(&buf[..read]));
            if text.contains("\r\n\r\n") {
                return;
            }
        }
    })
    .await;
    assert!(headers.is_ok(), "timed out waiting for SSE headers");
    assert!(text.contains("text/event-stream"), "got: {text}");

    fs::write(src.join("styles/main.css"), "body { color: red; }\n").unwrap();

    // read until an event arrives
    let event = timeout(Duration::from_secs(10), async {
        loop {
            if let Some(event) = text.lines().find_map(|line| line.strip_prefix("event: ")) {
                return event.to_string();
            }

            let read = stream.read(&mut buf).await.unwrap();
            assert!(read > 0, "stream closed before an event");
            text.push_str(&String::from_utf8_lossy(&buf[..read]));
        }
    })
    .await;
    let event = event.expect("timed out waiting for a reload event");

    // full page reload, not css hot-reload
    assert_eq!(event, "page", "hashed css must reload the full page");

    handle.abort();
}

// `watch_serve` serves under the build base: the config base is left empty
// on purpose, the served prefix must follow `BuildOptions::base`.
#[tokio::test]
async fn watch_serve_serves_under_build_base() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        "<html><head><link rel=\"stylesheet\" href=\"/app/styles/main.css\"></head>\
         <body>v1</body></html>",
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();

    let config = ServerConfig {
        dir: dir.path().join("dist"),
        ip: IpAddr::from([127, 0, 0, 1]),
        port: 0,
        headers: vec![],
        index: "index.html".into(),
        base: String::new(),
    };
    let options = BuildOptions {
        base: Some("/app".into()),
        ..BuildOptions::PROD
    };

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let server = watch_serve(&src, &config, &options, tx_log, &[])
        .await
        .unwrap();
    assert!(server.url.ends_with("/app"), "got: {}", server.url);

    let url = server.url.clone();
    let handle = tokio::spawn(async move { server.run().await });

    let body = reqwest::get(format!("{url}/"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains(">v1"), "got: {body}");
    assert!(
        body.contains("EventSource('/app/livereload')"),
        "got: {body}"
    );

    // the page is bundled and rewritten with the base prefix
    let hashed_css = fs::read_dir(config.dir.join("styles"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .find(|name| name.ends_with(".css"))
        .expect("a hashed css output");
    assert!(
        body.contains(&format!("/app/styles/{hashed_css}")),
        "got: {body}"
    );

    // nothing injected into the written files
    let on_disk = fs::read_to_string(config.dir.join("index.html")).unwrap();
    assert!(on_disk.contains(&format!("/app/styles/{hashed_css}")));
    assert!(!on_disk.contains("livereload"), "got: {on_disk}");

    handle.abort();
}
