#![cfg(feature = "watch")]

use std::{fs, path::Path, time::Duration};

use tempfile::tempdir;
use tokio::{
    sync::broadcast,
    time::{sleep, timeout},
};

use bundla::{BuildCause, BuildOptions, ReloadType, WatchLog, watch};

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
#[cfg(feature = "css")]
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
