use std::{fs, path::Path};

use tempfile::tempdir;

use bundla::{BundlerOptions, bundle};

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
async fn dev_bundle_mirrors_and_rewrites() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(
        &src,
        &[
            ("index.html", PAGE),
            ("styles/main.css", "body { color: black; }\n"),
            ("scripts/app.js", "console.log('app');\n"),
        ],
    );

    let dist = dir.path().join("dist");
    bundle(&src, &dist, BundlerOptions::DEV).await.unwrap();

    let out_html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(out_html.contains("./styles/main.css"));
    assert!(out_html.contains("./scripts/app.js"));
    assert!(dist.join("styles/main.css").is_file());
    assert!(dist.join("styles/main.map").is_file());
    assert!(dist.join("scripts/app.js").is_file());
    assert!(dist.join("scripts/app.js.map").is_file());
}

#[tokio::test]
async fn release_bundle_minifies_and_hashes() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(
        &src,
        &[
            ("index.html", PAGE),
            ("styles/main.css", "body { color: black; }\n"),
            ("scripts/app.js", "console.log('app');\n"),
        ],
    );

    let dist = dir.path().join("dist");
    bundle(&src, &dist, BundlerOptions::RELEASE).await.unwrap();

    let styles = dir_names(&dist.join("styles"));
    assert_eq!(
        styles.len(),
        1,
        "expected exactly one css output: {styles:?}"
    );
    let css_name = &styles[0];
    assert!(
        css_name.starts_with("main-") && css_name.ends_with(".css"),
        "{css_name}"
    );

    let scripts = dir_names(&dist.join("scripts"));
    assert_eq!(
        scripts.len(),
        1,
        "expected exactly one js output: {scripts:?}"
    );
    let js_name = &scripts[0];
    assert!(
        js_name.starts_with("app-") && js_name.ends_with(".js"),
        "{js_name}"
    );

    let out_html = fs::read_to_string(dist.join("index.html")).unwrap();
    assert!(out_html.contains(css_name));
    assert!(out_html.contains(js_name));
    assert!(!out_html.contains("main.css"));
    assert!(!out_html.contains("app.js"));
}

#[tokio::test]
async fn shared_asset_bundled_once_across_pages() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(
        &src,
        &[
            ("index.html", PAGE),
            (
                "about.html",
                "<link rel=\"stylesheet\" href=\"./styles/main.css\" />",
            ),
            ("styles/main.css", "body { color: black; }\n"),
            ("scripts/app.js", "console.log('app');\n"),
        ],
    );

    let dist = dir.path().join("dist");
    bundle(&src, &dist, BundlerOptions::RELEASE).await.unwrap();

    let styles = dir_names(&dist.join("styles"));
    assert_eq!(
        styles.len(),
        1,
        "expected a single shared css output: {styles:?}"
    );

    let scripts = dir_names(&dist.join("scripts"));
    assert_eq!(
        scripts.len(),
        1,
        "expected a single shared js output: {scripts:?}"
    );

    let index_html = fs::read_to_string(dist.join("index.html")).unwrap();
    let about_html = fs::read_to_string(dist.join("about.html")).unwrap();
    assert!(index_html.contains(&styles[0]));
    assert!(about_html.contains(&styles[0]));
    assert!(index_html.contains(&scripts[0]));
}

#[tokio::test]
async fn bundle_errors_no_html_pages() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("styles/main.css", "body{}\n")]);
    let dist = dir.path().join("dist");

    let err = bundle(&src, &dist, BundlerOptions::DEV).await.unwrap_err();
    assert!(err.contains("no .html pages"), "got: {err}");
}

#[tokio::test]
async fn bundle_errors_missing_src() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");

    let err = bundle(&dir.path().join("missing"), &dist, BundlerOptions::DEV)
        .await
        .unwrap_err();
    assert!(err.contains("source directory not found"), "got: {err}");
}

#[tokio::test]
async fn bundle_errors_dist_equal_src() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("index.html", PAGE)]);

    let err = bundle(&src, &src, BundlerOptions::DEV).await.unwrap_err();
    assert!(
        err.contains("must not be the source directory"),
        "got: {err}"
    );
}

#[tokio::test]
async fn bundle_errors_dist_inside_src() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    write_files(&src, &[("index.html", PAGE)]);

    let err = bundle(&src, &src.join("out"), BundlerOptions::DEV)
        .await
        .unwrap_err();
    assert!(err.contains("must not be inside"), "got: {err}");
}
