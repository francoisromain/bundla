mod css;
mod js;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use super::Options;

use css::css_bundle;
use js::js_bundle;

/// bundled css extensions, lowercased
pub const CSS_EXTENSIONS: [&str; 1] = ["css"];

/// bundled js extensions, lowercased
pub const JS_EXTENSIONS: [&str; 6] = ["js", "mjs", "cjs", "ts", "mts", "cts"];

pub async fn asset_process(
    asset_path: &Path,
    src: &Path,
    dist: &Path,
    options: Options,
    cache: &mut HashMap<PathBuf, String>,
) -> Result<String, String> {
    let asset_path_relative = asset_path
        .strip_prefix(src)
        .expect("canonicalized asset path is always under src");
    let ext = asset_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let dir = asset_path_relative
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let file_name = match ext.as_str() {
        e if CSS_EXTENSIONS.contains(&e) => css_bundle(asset_path, src, dist, dir, options, cache)?,
        e if JS_EXTENSIONS.contains(&e) => {
            js_bundle(asset_path, src, asset_path_relative, dist, dir, options).await?
        }
        other => {
            return Err(format!(
                "unsupported asset type: .{} ({})",
                other,
                asset_path.display()
            ));
        }
    };

    Ok(file_name)
}

// the source file stem, falling back to a fixed name for unusual names
fn file_stem_extract(source: &Path, fallback: &str) -> String {
    source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(fallback)
        .to_string()
}

// output directory for a relative `dir` under `dist`, created on the fly
fn dist_mkdir(dist: &Path, dir: &Path) -> Result<PathBuf, String> {
    let out = if dir.as_os_str().is_empty() {
        dist.to_path_buf()
    } else {
        dist.join(dir)
    };
    fs::create_dir_all(&out).map_err(|err| format!("failed to create {}: {err}", out.display()))?;

    Ok(out)
}

// `name` with an optional content hash, preserving the source directory
fn file_name_output_format(stem: &str, ext: &str, bytes: &[u8], options: Options) -> String {
    if options.hash {
        format!("{stem}-{}.{ext}", hash_create(bytes))
    } else {
        format!("{stem}.{ext}")
    }
}

// std-only FNV-1a 64-bit hash, formatted as 16 hex chars
fn hash_create(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::super::Options;
    use super::*;

    #[test]
    fn hash_create_matches_fnv1a_vectors() {
        assert_eq!(hash_create(b""), "cbf29ce484222325");
        assert_eq!(hash_create(b"a"), "af63dc4c8601ec8c");
    }

    #[test]
    fn hash_create_is_stable_and_distinct() {
        assert_eq!(hash_create(b"css"), hash_create(b"css"));
        assert_ne!(hash_create(b"css"), hash_create(b"js"));
    }

    #[test]
    fn file_name_output_format_with_hash() {
        let options = Options {
            minify: false,
            sourcemap: false,
            hash: true,
        };

        let name = file_name_output_format("main", "js", b"body", options);
        assert_eq!(name, format!("main-{}.js", hash_create(b"body")));
    }

    #[test]
    fn file_name_output_format_without_hash() {
        let options = Options {
            minify: false,
            sourcemap: false,
            hash: false,
        };

        assert_eq!(
            file_name_output_format("main", "css", b"body", options),
            "main.css"
        );
    }

    #[test]
    fn file_stem_extract_normal() {
        assert_eq!(
            file_stem_extract(Path::new("styles/main.css"), "styles"),
            "main"
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_stem_extract_non_utf8_fallback() {
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(std::ffi::OsStr::from_bytes(&[0xff]));
        assert_eq!(file_stem_extract(path, "styles"), "styles");
    }

    #[test]
    fn dist_mkdir_root_dir_resolves_to_dist() {
        let dir = tempdir().unwrap();
        let dist = dir.path().join("out");

        let out = dist_mkdir(&dist, Path::new("")).unwrap();
        assert_eq!(out, dist);
        assert!(dist.is_dir());
    }

    #[test]
    fn dist_mkdir_nested_dir_creates_subtree() {
        let dir = tempdir().unwrap();
        let dist = dir.path().join("out");

        let out = dist_mkdir(&dist, Path::new("a/b")).unwrap();
        assert_eq!(out, dist.join("a/b"));
        assert!(dist.join("a/b").is_dir());
    }

    #[tokio::test]
    async fn asset_process_extension_is_case_insensitive() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let dist = src.join("out");
        fs::write(src.join("THEME.CSS"), "body { color: red; }").unwrap();

        let name = asset_process(
            &src.join("THEME.CSS"),
            &src,
            &dist,
            Options::DEV,
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(name, "THEME.css");
        assert!(dist.join("THEME.css").is_file());
        assert!(dist.join("THEME.map").is_file());
    }

    #[tokio::test]
    async fn asset_process_dispatch_css() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let dist = src.join("out");
        let styles = src.join("styles");
        fs::create_dir_all(&styles).unwrap();
        fs::write(styles.join("theme.css"), "body { color: red; }").unwrap();

        let name = asset_process(
            &styles.join("theme.css"),
            &src,
            &dist,
            Options::DEV,
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(name, "theme.css");
        assert!(dist.join("styles/theme.css").is_file());
        assert!(dist.join("styles/theme.map").is_file());
    }

    #[tokio::test]
    async fn asset_process_dispatch_js() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let dist = src.join("out");
        let scripts = src.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("app.js"), "console.log('hi');\n").unwrap();

        let name = asset_process(
            &scripts.join("app.js"),
            &src,
            &dist,
            Options::DEV,
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(name, "app.js");
        assert!(dist.join("scripts/app.js").is_file());
        assert!(dist.join("scripts/app.js.map").is_file());
    }

    #[tokio::test]
    async fn asset_process_unsupported_extension() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let dist = src.join("out");
        fs::write(src.join("image.png"), "png").unwrap();

        let err = asset_process(
            &src.join("image.png"),
            &src,
            &dist,
            Options::DEV,
            &mut HashMap::new(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("unsupported asset type"), "got: {err}");
    }

    #[tokio::test]
    async fn js_hash_sourcemap_combo_emits_entry_and_map() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let dist = src.join("out");
        let scripts = src.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("app.js"), "console.log('hi');\n").unwrap();

        let options = Options {
            minify: true,
            sourcemap: true,
            hash: true,
        };
        let name = asset_process(
            &scripts.join("app.js"),
            &src,
            &dist,
            options,
            &mut HashMap::new(),
        )
        .await
        .unwrap();

        let dist_dir = dist.join("scripts");
        assert!(dist_dir.join(&name).is_file(), "missing {}", name);
        let maps: Vec<String> = fs::read_dir(&dist_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".map"))
            .collect();
        assert!(
            !maps.is_empty(),
            "expected a sourcemap in {}",
            dist_dir.display()
        );
    }
}
