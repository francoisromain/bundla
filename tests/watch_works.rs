use std::{fs, net::IpAddr, path::Path, time::Duration};

use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::broadcast,
    time::{sleep, timeout},
};

use bundla::{BundlerOptions, ServerConfig, WatchLog, watch, watch_serve};

fn page(body: &str) -> String {
    format!("<html><body>{body}</body></html>")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

// Poll `path` until it contains `needle`, or fail after 10s
// (the watcher debounces changes for 200ms before rebundling)
async fn wait_contains(path: &Path, needle: &str) {
    let waited = timeout(Duration::from_secs(10), async {
        loop {
            if read(path).contains(needle) {
                return;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(
        waited.is_ok(),
        "timed out waiting for {needle:?} in {}",
        path.display()
    );
}

#[tokio::test]
async fn watch_bundles_on_startup_and_rebundles_on_change() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();

    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, BundlerOptions::DEV, tx_log)
        .await
        .unwrap();

    // bundled once on startup, before `watch` returns
    assert!(read(&dist.join("index.html")).contains("v1"));

    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("index.html"), page("v2")).unwrap();
    wait_contains(&dist.join("index.html"), "v2").await;

    // the change was reported on the log channel
    let mut saw_change = false;
    while let Ok(log) = rx_log.try_recv() {
        if matches!(log, WatchLog::Change(..)) {
            saw_change = true;
        }
    }
    assert!(saw_change, "expected a WatchLog::Change on the log channel");

    // nothing injected into the written files: no reload script
    let html = read(&dist.join("index.html"));
    assert!(!html.contains("EventSource"), "got: {html}");
    assert!(!html.contains("livereload"), "got: {html}");
    assert_eq!(html.matches("<script").count(), 0, "got: {html}");

    handle.abort();
}

#[tokio::test]
async fn watch_keeps_previous_output_on_failed_rebundle() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();

    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, BundlerOptions::DEV, tx_log)
        .await
        .unwrap();
    let handle = tokio::spawn(watcher.run());

    // remove the only page: the rebundle fails
    fs::remove_file(src.join("index.html")).unwrap();

    let errored = timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(WatchLog::Error(_)) = rx_log.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(
        errored.is_ok(),
        "expected a WatchLog::Error on the log channel"
    );

    // the previous bundle is kept
    assert!(read(&dist.join("index.html")).contains("v1"));

    handle.abort();
}

#[tokio::test]
async fn watch_missing_src_errs() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("nowhere");
    let dist = dir.path().join("dev");

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let err = watch(&src, &dist, BundlerOptions::DEV, tx_log)
        .await
        .unwrap_err();
    assert!(err.contains("failed to watch"), "got: {err}");
}

// A hashed profile (`-sw`) must reload the full page on css edits: css
// hot-reload re-fetches the stylesheet href held by the page, and the
// rebuild serves a new hashed name — the stale href would 404. The
// wire format is `event: <name>` on the SSE stream.
#[tokio::test]
async fn dev_hashed_css_change_reloads_full_page() {
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
    };

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let server = watch_serve(&src, &config, BundlerOptions::PROD, tx_log)
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
