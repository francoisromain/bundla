// The CLI always builds with serving (`-s`), so its tests need the feature
#![cfg(feature = "serve")]

use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{ChildStdout, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use tempfile::tempdir;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bundla"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

#[test]
fn help_exits_zero() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bundle src into dist"), "got: {stdout}");
}

#[test]
fn version_exits_zero() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "got: {stdout}");
}

#[test]
fn help_documents_random_port_default() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("random free port"), "got: {stdout}");
}

#[test]
fn invalid_port_rejected() {
    let out = run(&["-s", "--port", "99999"]);
    assert_eq!(out.status.code(), Some(2), "clap parse errors exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid value") || stderr.contains("error:"),
        "got: {stderr}"
    );
}

// Server-only flags are rejected at parse time without --serve
#[test]
fn server_flags_require_serve() {
    for flag in [
        vec!["--open"],
        vec!["--header", "X-Engine: bundla"],
        vec!["--port", "9000"],
        vec!["--ip", "0.0.0.0"],
        vec!["--index", "home.html"],
    ] {
        let out = run(&flag);
        assert_eq!(
            out.status.code(),
            Some(2),
            "clap requires errors exit 2: {flag:?}"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("required arguments were not provided"),
            "got: {stderr}"
        );
        assert!(stderr.contains("--serve"), "got: {stderr}");
    }
}

// Poll until `path` exists and contains `needle`, or give up on timeout
fn wait_contains(path: &Path, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(text) = fs::read_to_string(path)
            && text.contains(needle)
        {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

// Read child stdout lines until the `Serving <url>` line
fn serving_url(stdout: ChildStdout) -> String {
    for line in BufReader::new(stdout).lines() {
        let line = line.unwrap();
        if let Some(url) = line.strip_prefix("Serving ") {
            return url.to_string();
        }
    }
    panic!("child exited before printing its serving url");
}

// The single hashed css file under `styles`
fn hashed_css_name(styles: &Path) -> String {
    fs::read_dir(styles)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .find(|n| n.ends_with(".css"))
        .expect("a css output in styles")
}

fn fixture(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        r#"<html><head><link rel="stylesheet" href="./styles/main.css" /></head><body>v1</body></html>"#,
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
}

// Bare `bundla`: prod build, hashed filenames, then exit
#[test]
fn bare_builds_prod() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let dist = dir.path().join("out");

    let src_arg = dir.path().join("src").to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let out = run(&["--src", &src_arg, "--dist", &dist_arg]);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bundled"), "got: {stdout}");

    // prod profile: hashed css, no sourcemap
    let css = hashed_css_name(&dist.join("styles"));
    assert!(css.starts_with("main-"), "got: {css}");
    assert!(!dist.join("styles/main.css.map").exists());
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(html.contains(&css), "got: {html}");
}

// `-d`: dev build, unhashed filenames with sourcemaps, then exit
#[test]
fn dev_builds_dev() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let dist = dir.path().join("out");

    let src_arg = dir.path().join("src").to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let out = run(&["-d", "--src", &src_arg, "--dist", &dist_arg]);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bundled"), "got: {stdout}");

    // dev profile: unhashed css + sourcemap
    assert_eq!(hashed_css_name(&dist.join("styles")), "main.css");
    assert!(dist.join("styles/main.css.map").is_file());
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(html.contains("./styles/main.css"), "got: {html}");
}

// `--base`: root-relative references under it are bundled and rewritten
// with the base prefix
#[test]
fn base_builds_root_relative_refs() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        r#"<html><head><link rel="stylesheet" href="/app/styles/main.css" /></head><body>v1</body></html>"#,
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
    let dist = dir.path().join("out");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let out = run(&["--base", "/app", "--src", &src_arg, "--dist", &dist_arg]);

    assert!(out.status.success());
    let css = hashed_css_name(&dist.join("styles"));
    assert!(css.starts_with("main-"), "got: {css}");
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(html.contains(&format!("/app/styles/{css}")), "got: {html}");
}

// `--base` must start with `/`
#[test]
fn base_rejects_relative_values() {
    let out = run(&["--base", "app"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("must start with `/`"), "got: {stderr}");
}

#[test]
fn base_rejects_empty_value() {
    let out = run(&["--base", ""]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("must not be empty"), "got: {stderr}");
}

#[test]
fn base_rejects_invalid_values() {
    for value in ["//app", "/app//x", "/app/../x", "/app/*"] {
        let out = run(&["--base", value]);
        assert_eq!(out.status.code(), Some(2), "value: {value:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("invalid value") || stderr.contains("error:"),
            "value: {value:?}, got: {stderr}"
        );
    }
}

// `--base /app/` is normalized to `/app` for the build and the server
#[test]
fn base_normalizes_trailing_slash() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        r#"<html><head><link rel="stylesheet" href="/app/styles/main.css" /></head><body>v1</body></html>"#,
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
    let dist = dir.path().join("out");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let out = run(&["--base", "/app/", "--src", &src_arg, "--dist", &dist_arg]);

    assert!(out.status.success());
    let css = hashed_css_name(&dist.join("styles"));
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(html.contains(&format!("/app/styles/{css}")), "got: {html}");
}

// Headless, end to end: `-dw` builds on startup, rebuilds on change,
// and writes clean files (no reload script injection)
#[test]
fn dev_watch_rebuilds_without_injection() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), "<html><body>v1</body></html>").unwrap();
    let dist = dir.path().join("out");
    let out_html = dist.join("index.html");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let mut child = bin()
        .args(["-dw", "--src", &src_arg, "--dist", &dist_arg])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    assert!(
        wait_contains(&out_html, "v1", Duration::from_secs(60)),
        "initial build missing"
    );

    fs::write(src.join("index.html"), "<html><body>v2</body></html>").unwrap();
    assert!(
        wait_contains(&out_html, "v2", Duration::from_secs(60)),
        "rebuild after change missing"
    );

    // nothing injected into the written files: no reload script
    let html = fs::read_to_string(&out_html).unwrap();
    assert!(!html.contains("EventSource"), "got: {html}");
    assert!(!html.contains("livereload"), "got: {html}");
    assert_eq!(html.matches("<script").count(), 0, "got: {html}");

    child.kill().unwrap();
    child.wait().unwrap();
}

// `-w` keeps hashing while rebuilding headless
#[test]
fn watch_hashes_on_rebuild() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let dist = dir.path().join("out");
    let styles_dir = dist.join("styles");
    let src = dir.path().join("src");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let mut child = bin()
        .args(["-w", "--src", &src_arg, "--dist", &dist_arg])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    assert!(
        wait_contains(&dist.join("index.html"), "v1", Duration::from_secs(60)),
        "initial build missing"
    );
    let first_css = hashed_css_name(&styles_dir);
    assert!(first_css.starts_with("main-"), "got: {first_css}");

    // css edit: the content hash must change
    fs::write(src.join("styles/main.css"), "body { color: red; }\n").unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) && hashed_css_name(&styles_dir) == first_css {
        thread::sleep(Duration::from_millis(100));
    }
    assert_ne!(
        hashed_css_name(&styles_dir),
        first_css,
        "rebuild after css change missing"
    );

    // nothing injected into the written files: no reload script
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(!html.contains("EventSource"), "got: {html}");
    assert!(!html.contains("livereload"), "got: {html}");
    assert_eq!(html.matches("<script").count(), 0, "got: {html}");

    child.kill().unwrap();
    child.wait().unwrap();
}

// `-s`: build once, then serve the output statically
#[tokio::test]
async fn serve_builds_then_serves() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), "<html><body>v1</body></html>").unwrap();
    let dist = dir.path().join("out");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let mut child = bin()
        .args(["-s", "--src", &src_arg, "--dist", &dist_arg])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let url = serving_url(child.stdout.take().unwrap());

    // the build ran before the server started, and the files stay clean
    let on_disk = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(on_disk.contains("v1"));
    assert!(!on_disk.contains("livereload"), "got: {on_disk}");

    let body = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(body.contains("v1"), "got: {body}");

    child.kill().unwrap();
    child.wait().unwrap();
}

// `-s --base /app`: the build is rewritten under the prefix, and the
// built-in server honors it (redirect + page + hashed asset)
#[tokio::test]
async fn serve_honors_base() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("styles")).unwrap();
    fs::write(
        src.join("index.html"),
        r#"<html><head><link rel="stylesheet" href="/app/styles/main.css" /></head><body>v1</body></html>"#,
    )
    .unwrap();
    fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
    let dist = dir.path().join("out");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let mut child = bin()
        .args([
            "-s", "--base", "/app", "--src", &src_arg, "--dist", &dist_arg,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let url = serving_url(child.stdout.take().unwrap());
    assert!(url.ends_with("/app"), "got: {url}");

    // the written page is clean and rewritten under the base
    let css = hashed_css_name(&dist.join("styles"));
    let html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(html.contains(&format!("/app/styles/{css}")), "got: {html}");
    assert!(!html.contains("livereload"), "got: {html}");

    // `/app` redirects to `/app/` so relative URLs resolve under the base
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let redirect = client.get(&url).send().await.unwrap();
    assert_eq!(redirect.status(), 308);
    assert_eq!(
        redirect
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "/app/"
    );

    // the page and the hashed asset are served under the base
    let body = reqwest::get(format!("{url}/"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("v1"), "got: {body}");
    assert!(body.contains(&format!("/app/styles/{css}")), "got: {body}");

    let asset = reqwest::get(format!("{url}/styles/{css}")).await.unwrap();
    assert_eq!(asset.status(), 200);
    assert!(!asset.text().await.unwrap().is_empty());

    // outside the base is not served
    let root = url.strip_suffix("/app").unwrap();
    assert_eq!(
        reqwest::get(format!("{root}/")).await.unwrap().status(),
        404
    );

    child.kill().unwrap();
    child.wait().unwrap();
}

// `-dws` end to end: watch + serve with live reload — the reload script
// is injected into HTTP responses, never written to the output files
#[tokio::test]
async fn watch_serve_reloads_on_change() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), "<html><body>v1</body></html>").unwrap();
    let dist = dir.path().join("out");

    let src_arg = src.to_string_lossy().to_string();
    let dist_arg = dist.to_string_lossy().to_string();
    let mut child = bin()
        .args(["-dws", "--src", &src_arg, "--dist", &dist_arg])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let url = serving_url(child.stdout.take().unwrap());

    // the reload script is served over HTTP…
    let body = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(body.contains("v1"), "got: {body}");
    assert!(body.contains("livereload"), "got: {body}");

    fs::write(src.join("index.html"), "<html><body>v2</body></html>").unwrap();

    // …the change triggers a rebuild, and the server serves the fresh page
    let reloaded = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let body = reqwest::get(&url).await.unwrap().text().await.unwrap();
            if body.contains("v2") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(reloaded.is_ok(), "page not rebuilt and served after change");

    // …but never into the written files
    let on_disk = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(!on_disk.contains("livereload"), "got: {on_disk}");

    child.kill().unwrap();
    child.wait().unwrap();
}
