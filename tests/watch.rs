use std::{fs, path::Path, time::Duration};

use tempfile::tempdir;
use tokio::{
    sync::broadcast,
    time::{sleep, timeout},
};

use bundla::{BuildCause, BuildOptions, ReloadType, WatchLog, watch};

#[cfg(feature = "serve")]
use std::net::IpAddr;

#[cfg(feature = "serve")]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[cfg(feature = "serve")]
use bundla::{ServerConfig, watch_serve};

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

// The build that runs before `watch` returns reports a full page reload with
// no changed paths and `BuildCause::Startup`. It is already queued when
// `watch` resolves.
async fn assert_startup_built(rx_log: &mut broadcast::Receiver<WatchLog>) {
    let log = timeout(Duration::from_secs(10), rx_log.recv())
        .await
        .expect("timed out waiting for the startup build log")
        .expect("log channel closed");
    match log {
        WatchLog::Built {
            cause,
            reload_type,
            paths,
        } => {
            assert_eq!(cause, BuildCause::Startup);
            assert_eq!(reload_type, ReloadType::Page);
            assert!(paths.is_empty(), "got: {paths:?}");
        }
        other => panic!("expected a startup WatchLog::Built, got: {other:?}"),
    }
}

#[tokio::test]
async fn watch_builds_on_startup_and_rebuilds_on_change() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();

    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[])
        .await
        .unwrap();

    // bundled once on startup, before `watch` returns
    assert!(read(&dist.join("index.html")).contains("v1"));

    // the startup build is logged too, with no changed paths
    assert_startup_built(&mut rx_log).await;

    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("index.html"), page("v2")).unwrap();

    // reported only once the new `dist` is in place
    let log = timeout(Duration::from_secs(10), rx_log.recv())
        .await
        .expect("timed out waiting for a build log")
        .expect("log channel closed");
    match log {
        WatchLog::Built {
            cause,
            reload_type,
            paths,
        } => {
            assert_eq!(cause, BuildCause::Change);
            assert_eq!(reload_type, ReloadType::Page);
            assert!(
                paths.iter().any(|path| path.ends_with("index.html")),
                "got: {paths:?}"
            );
        }
        other => panic!("expected WatchLog::Built, got: {other:?}"),
    }
    assert!(read(&dist.join("index.html")).contains("v2"));

    // nothing injected into the written files: no reload script
    let html = read(&dist.join("index.html"));
    assert!(!html.contains("EventSource"), "got: {html}");
    assert!(!html.contains("livereload"), "got: {html}");
    assert_eq!(html.matches("<script").count(), 0, "got: {html}");

    handle.abort();
}

#[tokio::test]
async fn watch_keeps_previous_output_on_failed_rebuild() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();

    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[])
        .await
        .unwrap();
    assert_startup_built(&mut rx_log).await;
    let handle = tokio::spawn(watcher.run());

    // remove the only page: the rebundle fails
    fs::remove_file(src.join("index.html")).unwrap();

    let errored = timeout(Duration::from_secs(10), async {
        loop {
            match rx_log.recv().await {
                Ok(WatchLog::Error(_)) => return,
                Ok(WatchLog::Built { .. }) => {
                    panic!("a failed build must not report WatchLog::Built")
                }
                Ok(_) => {}
                Err(err) => panic!("log channel closed: {err}"),
            }
        }
    })
    .await;
    assert!(
        errored.is_ok(),
        "expected a WatchLog::Error on the log channel"
    );

    // the previous build is kept
    assert!(read(&dist.join("index.html")).contains("v1"));

    handle.abort();
}

// The log carries the browser reload type: hashed css is a page reload
#[tokio::test]
async fn watch_log_reports_page_for_hashed_css() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dist");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        "<html><head><link rel=\"stylesheet\" href=\"./styles/main.css\"></head>\
         <body>v1</body></html>",
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();

    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::PROD, tx_log, &[])
        .await
        .unwrap();
    assert_startup_built(&mut rx_log).await;
    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("styles/main.css"), "body { color: red; }\n").unwrap();

    let log = timeout(Duration::from_secs(10), rx_log.recv())
        .await
        .expect("timed out waiting for a build log")
        .expect("log channel closed");
    match log {
        WatchLog::Built {
            cause,
            reload_type,
            paths,
        } => {
            assert_eq!(cause, BuildCause::Change);
            assert_eq!(reload_type, ReloadType::Page);
            assert!(
                paths.iter().any(|path| path.ends_with("main.css")),
                "got: {paths:?}"
            );
        }
        other => panic!("expected WatchLog::Built, got: {other:?}"),
    }

    handle.abort();
}

#[tokio::test]
async fn watch_missing_src_errs() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("nowhere");
    let dist = dir.path().join("dev");

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let err = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[])
        .await
        .unwrap_err();
    assert!(err.contains("failed to watch"), "got: {err}");
}

// No filter: every file under src triggers a build (static assets included)
#[tokio::test]
async fn watch_watches_every_file_by_default() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();
    fs::write(src.join("data.txt"), "v1").unwrap();

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[])
        .await
        .unwrap();
    assert_eq!(read(&dist.join("data.txt")), "v1");

    let handle = tokio::spawn(watcher.run());
    fs::write(src.join("data.txt"), "v2").unwrap();
    wait_contains(&dist.join("data.txt"), "v2").await;

    handle.abort();
}

// With a filter, an unlisted extension is ignored while a listed one rebuilds
#[tokio::test]
async fn watch_filter_ignores_unlisted_extensions() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();
    fs::write(src.join("data.txt"), "v1").unwrap();

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &["html"])
        .await
        .unwrap();
    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("data.txt"), "v2").unwrap();
    sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        read(&dist.join("data.txt")),
        "v1",
        "a filtered-out change must not build"
    );

    fs::write(src.join("index.html"), page("v2")).unwrap();
    wait_contains(&dist.join("index.html"), "v2").await;

    handle.abort();
}

// Filter values are lowercased, on both sides
#[tokio::test]
async fn watch_filter_matches_case_insensitively() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();
    fs::write(src.join("data.txt"), "v1").unwrap();

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &["TXT"])
        .await
        .unwrap();
    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("data.txt"), "v2").unwrap();
    wait_contains(&dist.join("data.txt"), "v2").await;

    handle.abort();
}

// A filter item also matches an exact file name, not only an extension
#[tokio::test]
async fn watch_filter_matches_exact_file_name() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();
    fs::write(src.join("data.txt"), "v1").unwrap();
    fs::write(src.join("other.txt"), "v1").unwrap();

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &["data.txt"])
        .await
        .unwrap();
    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("other.txt"), "v2").unwrap();
    sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        read(&dist.join("other.txt")),
        "v1",
        "`data.txt` must not match `other.txt`"
    );

    fs::write(src.join("data.txt"), "v2").unwrap();
    wait_contains(&dist.join("data.txt"), "v2").await;

    handle.abort();
}

// `&[""]` means "watch all", like `&[]`
#[tokio::test]
async fn watch_filter_empty_string_watches_all() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();
    fs::write(src.join("data.txt"), "v1").unwrap();

    let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
    let watcher = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[""])
        .await
        .unwrap();
    let handle = tokio::spawn(watcher.run());

    fs::write(src.join("data.txt"), "v2").unwrap();
    wait_contains(&dist.join("data.txt"), "v2").await;

    handle.abort();
}

// Invalid filter values are reported by `watch`, not silently ignored
#[tokio::test]
async fn watch_filter_rejects_invalid_values() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    let dist = dir.path().join("dev");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), page("v1")).unwrap();

    for (filter, expected) in [
        (".html", "must not start with a dot"),
        ("src/index.html", "must not contain a path separator"),
    ] {
        let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
        let err = watch(&src, &dist, &BuildOptions::DEV, tx_log, &[filter])
            .await
            .unwrap_err();
        assert!(err.contains("failed to watch"), "got: {err}");
        assert!(err.contains(expected), "filter {filter:?}: got: {err}");
    }
}

// A hashed profile (`-sw`) must reload the full page on css edits: css
// hot-reload re-fetches the stylesheet href held by the page, and the
// build serves a new hashed name — the stale href would 404. The
// wire format is `event: <name>` on the SSE stream.
#[cfg(feature = "serve")]
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
#[cfg(feature = "serve")]
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
    let on_disk = read(&config.dir.join("index.html"));
    assert!(on_disk.contains(&format!("/app/styles/{hashed_css}")));
    assert!(!on_disk.contains("livereload"), "got: {on_disk}");

    handle.abort();
}
