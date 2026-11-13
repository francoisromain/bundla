mod assets;
mod html;
mod path;
mod static_assets;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use assets::asset_process;
use html::{html_asset_refs_extract, html_minify, html_rewrite};
use path::{asset_ref_path_resolve, asset_ref_rewrite, dist_guard};
use static_assets::static_asset_copy;

/// Build options for the HTML, CSS, JS/TS output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BuildOptions {
    /// Minify the css, js, and output html.
    pub minify: bool,
    /// Emit `.map` sourcemap files next to the output.
    pub sourcemap: bool,
    /// Add a hash to output filenames (`main-<hash>.css`, `main-<hash>.js`).
    pub hash: bool,
    /// URL prefix where the output is served (e.g. `/app`).
    /// Root-relative references starting with this base are bundled.
    /// `None` leaves them for the browser.
    /// Use `webadev::base_normalize` to validate and normalize user input.
    pub base: Option<String>,
}

impl BuildOptions {
    /// Unminified, sourcemapped, no-hash filenames.
    ///
    /// ```
    /// use bundla::BuildOptions;
    ///
    /// assert!(!BuildOptions::DEV.minify);
    /// assert!(BuildOptions::DEV.sourcemap);
    /// assert!(!BuildOptions::DEV.hash);
    /// assert!(BuildOptions::DEV.base.is_none());
    /// ```
    pub const DEV: BuildOptions = BuildOptions {
        minify: false,
        sourcemap: true,
        hash: false,
        base: None,
    };

    /// Minified, hashed filenames, no sourcemaps.
    ///
    /// ```
    /// use bundla::BuildOptions;
    ///
    /// assert!(BuildOptions::PROD.minify);
    /// assert!(!BuildOptions::PROD.sourcemap);
    /// assert!(BuildOptions::PROD.hash);
    /// assert!(BuildOptions::PROD.base.is_none());
    /// ```
    pub const PROD: BuildOptions = BuildOptions {
        minify: true,
        sourcemap: false,
        hash: true,
        base: None,
    };

    /// URL prefix where the output is served.
    ///
    /// Empty when no base is set.
    /// Pass this to `ServerConfig.base` when building and serving manually.
    /// `watch_serve` applies it automatically and ignores the config base.
    pub fn server_base(&self) -> String {
        self.base.clone().unwrap_or_default()
    }
}

/// Build the HTML, CSS, and JS/TS assets from `src` into `dist`.
///
/// Every `.html` file under `src` is a page entry, mirrored to `dist`.
/// `<link rel="stylesheet">` and `<script type="module">` references are
/// bundled (one output per asset, cached by source path) and rewritten in
/// place.
/// `dist` is replaced atomically: the build renders into a temp sibling,
/// so a failed build never leaves a wiped or partial output.
///
/// `Ok` returns the non-fatal warnings collected during the build
/// (skipped unreadable assets, skipped non-module scripts, bundler warnings).
/// The caller decides how to display them.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
///
/// use bundla::{BuildOptions, build};
/// use tempfile::tempdir;
///
/// #[tokio::main]
/// async fn main() {
///     let dir = tempdir().unwrap();
///     let src = dir.path().join("src");
///     let dist = dir.path().join("dist");
///     fs::create_dir_all(src.join("styles")).unwrap();
///     fs::create_dir_all(src.join("scripts")).unwrap();
///     fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
///     fs::write(src.join("scripts/app.js"), "console.log('app');\n").unwrap();
///     fs::write(
///         src.join("index.html"),
///         "<html><head>\
///          <link rel=\"stylesheet\" href=\"./styles/main.css\">\
///          <script type=\"module\" src=\"./scripts/app.js\"></script>\
///          </head><body><p>hello</p></body></html>",
///     )
///     .unwrap();
///
///     let warnings = build(&src, &dist, &BuildOptions::PROD).await.unwrap();
///     assert!(warnings.is_empty(), "got: {warnings:?}");
///
///     let html = fs::read_to_string(dist.join("index.html")).unwrap();
///     assert!(html.contains("main-"), "expected a hashed css name: {html}");
///     assert!(html.contains("app-"), "expected a hashed js name: {html}");
/// }
/// ```
pub async fn build(src: &Path, dist: &Path, options: &BuildOptions) -> Result<Vec<String>, String> {
    if !src.is_dir() {
        return Err(format!("source directory not found: {}", src.display()));
    }

    let src = src
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", src.display()))?;

    dist_guard(&src, dist)?;

    let page_path_list = page_path_list_build(&src);

    if page_path_list.is_empty() {
        return Err(format!("no .html pages found in {}", src.display()));
    }

    let mut warnings = Vec::new();

    let dist_tmp = dist
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            ".{}.tmp-{}",
            dist.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
            std::process::id()
        ));

    if let Err(err) = static_asset_copy(&src, &dist_tmp, &mut warnings) {
        let _ = fs::remove_dir_all(&dist_tmp);

        return Err(err);
    }

    if let Err(err) =
        page_list_bundle(&page_path_list, &src, &dist_tmp, options, &mut warnings).await
    {
        let _ = fs::remove_dir_all(&dist_tmp);

        return Err(err);
    }

    if dist.exists() {
        fs::remove_dir_all(dist)
            .map_err(|err| format!("failed to clean {}: {err}", dist.display()))?;
    }

    fs::rename(&dist_tmp, dist)
        .map_err(|err| format!("failed to move build into {}: {err}", dist.display()))?;

    Ok(warnings)
}

async fn page_list_bundle(
    page_path_list: &[PathBuf],
    src: &Path,
    dist: &Path,
    options: &BuildOptions,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    fs::create_dir_all(dist)
        .map_err(|err| format!("failed to create {}: {err}", dist.display()))?;

    // Store the refs to the processed files for reuse in subsequent page
    let mut cache: HashMap<PathBuf, String> = HashMap::new();

    for page_path in page_path_list {
        page_bundle(page_path, src, dist, options, &mut cache, warnings).await?;
    }

    Ok(())
}

// Find every `.html` file under `dir`, recursively.
fn page_path_list_build(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        })
        .collect();
    pages.sort();

    pages
}

// Extract refs to the assets inside the page.
// If the asset is not in cache yet, process it, and store its ref in cache.
// Otherwise, reuse the ref to the asset in cache.
// Rewrite the html with refs to the processed assets.
async fn page_bundle(
    page_path: &Path,
    src: &Path,
    dist: &Path,
    options: &BuildOptions,
    cache: &mut HashMap<PathBuf, String>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let html = fs::read_to_string(page_path)
        .map_err(|err| format!("{} unreadable: {err}", page_path.display()))?;

    let asset_refs = html_asset_refs_extract(&html, page_path, warnings)?;

    let mut asset_ref_rewritten_map: HashMap<String, String> = HashMap::new();
    for asset_ref in asset_refs {
        if let Some(asset_path) =
            asset_ref_path_resolve(src, page_path, &asset_ref, options.base.as_deref())?
        {
            let file_name = match cache.get(&asset_path) {
                Some(file_name) => file_name.clone(),
                None => {
                    let name =
                        asset_process(&asset_path, src, dist, options, cache, warnings).await?;

                    cache.insert(asset_path.clone(), name.clone());
                    name
                }
            };
            let asset_ref_rewritten = asset_ref_rewrite(&asset_ref, &file_name);
            asset_ref_rewritten_map.insert(asset_ref, asset_ref_rewritten);
        } else if options.base.is_none() && path::is_root_relative(asset_ref.trim()) {
            warnings.push(path::asset_ref_skip_warn(&asset_ref, page_path));
        }
    }

    let html = html_rewrite(&html, &asset_ref_rewritten_map)?;
    let html = if options.minify {
        html_minify(&html)?
    } else {
        html
    };

    let out_page_path = dist.join(
        page_path
            .strip_prefix(src)
            .expect("page path is always under src"),
    );
    if let Some(parent) = out_page_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    fs::write(&out_page_path, html)
        .map_err(|err| format!("failed to write {}: {err}", out_page_path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn server_base_maps_none_and_some() {
        assert_eq!(BuildOptions::DEV.server_base(), "");

        let app = BuildOptions {
            base: Some("/app".into()),
            ..BuildOptions::DEV
        };
        assert_eq!(app.server_base(), "/app");

        let root = BuildOptions {
            base: Some("/".into()),
            ..BuildOptions::DEV
        };
        assert_eq!(root.server_base(), "/");
    }

    #[test]
    fn page_path_list_build_finds_and_sorts_html() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        for rel in [
            "index.html",
            "ABOUT.HTML",
            "a/b.html",
            "a/c.html",
            "styles.css",
            "noext",
            "a/b.txt",
        ] {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, "").unwrap();
        }

        let pages = page_path_list_build(&root);
        let names: Vec<String> = pages
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["ABOUT.HTML", "a/b.html", "a/c.html", "index.html"]);
    }

    #[test]
    fn page_path_list_build_empty_returns_empty() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();

        assert!(page_path_list_build(&root).is_empty());
    }

    #[tokio::test]
    async fn page_bundle_unreadable_page_errs() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        let dist = root.join("dist");
        fs::create_dir_all(&src).unwrap();

        let mut cache = HashMap::new();
        let err = page_bundle(
            &src.join("missing.html"),
            &src,
            &dist,
            &BuildOptions::PROD,
            &mut cache,
            &mut vec![],
        )
        .await
        .unwrap_err();
        assert!(err.contains("unreadable"), "got: {err}");
    }

    #[tokio::test]
    async fn page_bundle_prod_minifies_and_hashes() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        let dist = root.join("dist");
        fs::create_dir_all(src.join("styles")).unwrap();
        fs::create_dir_all(src.join("scripts")).unwrap();
        fs::write(src.join("styles/main.css"), "body { color: black; }\n").unwrap();
        fs::write(src.join("scripts/app.js"), "console.log('app');\n").unwrap();
        fs::write(
            src.join("index.html"),
            "<html>\n  <head>\n    <link rel=\"stylesheet\" href=\"./styles/main.css\" />\n    <script type=\"module\" src=\"./scripts/app.js\"></script>\n  </head>\n  <body>\n    <p>hello world</p>\n  </body>\n</html>\n",
        )
        .unwrap();
        let html_source = fs::read_to_string(src.join("index.html")).unwrap();

        let mut cache = HashMap::new();
        page_bundle(
            &src.join("index.html"),
            &src,
            &dist,
            &BuildOptions::PROD,
            &mut cache,
            &mut vec![],
        )
        .await
        .unwrap();

        let css_name = cache.get(&src.join("styles/main.css")).unwrap();
        assert!(css_name.starts_with("main-"), "got: {css_name}");
        let js_name = cache.get(&src.join("scripts/app.js")).unwrap();
        assert!(js_name.starts_with("app-"), "got: {js_name}");

        let out_html = fs::read_to_string(dist.join("index.html")).unwrap();
        assert!(out_html.contains(css_name));
        assert!(out_html.contains(js_name));
        assert!(!out_html.contains("main.css") && !out_html.contains("app.js"));
        assert!(
            out_html.len() < html_source.len(),
            "expected minified output, got: {out_html}"
        );
    }
}
