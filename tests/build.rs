#![cfg(feature = "build")]

use std::{fs, path::Path};

use tempfile::tempdir;

use bundla::{BuildOptions, build};

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let file = root.join(rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(file, content).unwrap();
    }
}

fn dir_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

const PAGE: &str = r#"<html><head>
  <link rel="stylesheet" href="./styles/main.css" />
  <script type="module" src="./scripts/app.js"></script>
</head><body>welcome</body></html>
"#;

#[tokio::test]
async fn build_errors_no_html_pages() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("styles/main.css", "body{}\n")]);
    let dist = dir.path().join("dist");

    let err = build(&src, &dist, &BuildOptions::DEV).await.unwrap_err();
    assert!(err.contains("no .html pages"), "got: {err}");
}

#[tokio::test]
async fn build_errors_missing_src() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");

    let err = build(&dir.path().join("missing"), &dist, &BuildOptions::DEV)
        .await
        .unwrap_err();
    assert!(err.contains("source directory not found"), "got: {err}");
}

#[tokio::test]
async fn build_errors_dist_equal_src() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("index.html", PAGE)]);

    let err = build(&src, &src, &BuildOptions::DEV).await.unwrap_err();
    assert!(
        err.contains("must not be the source directory"),
        "got: {err}"
    );
}

#[tokio::test]
async fn build_errors_dist_inside_src() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("index.html", PAGE)]);

    let err = build(&src, &src.join("out"), &BuildOptions::DEV)
        .await
        .unwrap_err();
    assert!(err.contains("must not be inside"), "got: {err}");
}

// Root-relative references outside the base are left verbatim for the
// browser, and the missing assets are not bundled.
#[tokio::test]
async fn base_refs_outside_base_left_verbatim() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(
        &src,
        &[
            (
                "index.html",
                r#"<html><head>
  <link rel="stylesheet" href="/styles/main.css" />
</head><body>outside</body></html>"#,
            ),
            ("styles/main.css", "body { color: black; }\n"),
        ],
    );

    let dist = dir.path().join("dist");
    let options = BuildOptions {
        base: Some("/app".into()),
        ..BuildOptions::PROD
    };
    let warnings = build(&src, &dist, &options).await.unwrap();
    assert!(warnings.is_empty(), "got: {warnings:?}");

    let out_html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(
        out_html.contains("/styles/main.css"),
        "ref outside the base must be left verbatim, got: {out_html}"
    );
    assert!(
        !dist.join("styles").exists(),
        "the outside asset must not be bundled"
    );
}

// Without a base, root-relative references are skipped with a warning:
// the referenced css/js is not bundled (and not copied either), so it
// would 404 when served.
#[tokio::test]
async fn root_relative_refs_without_base_warn() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(
        &src,
        &[
            (
                "index.html",
                r#"<html><head>
  <link rel="stylesheet" href="/styles/main.css" />
</head><body>plain</body></html>"#,
            ),
            ("styles/main.css", "body { color: black; }\n"),
        ],
    );

    let dist = dir.path().join("dist");
    let warnings = build(&src, &dist, &BuildOptions::PROD).await.unwrap();
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(
        warnings[0].contains("skipping root-relative reference"),
        "got: {}",
        warnings[0]
    );
    assert!(
        warnings[0].contains("/styles/main.css"),
        "got: {}",
        warnings[0]
    );

    // left verbatim, and nothing bundled for it
    let out_html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(out_html.contains("/styles/main.css"), "got: {out_html}");
    assert!(!dist.join("styles").exists(), "got: {:?}", dir_names(&dist));
}
