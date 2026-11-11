use std::{
    fs,
    path::{Component, Path, PathBuf},
};

// Reject `dist` if equal to, inside, or containing `src`.
pub fn dist_guard(src: &Path, dist: &Path) -> Result<(), String> {
    let resolved = dist_path_resolve(dist)?;
    if resolved == src {
        return Err(format!(
            "output directory must not be the source directory: {}",
            dist.display()
        ));
    }

    if resolved.starts_with(src) {
        return Err(format!(
            "output directory must not be inside the source directory: {} is inside {}",
            dist.display(),
            src.display()
        ));
    }

    if src.starts_with(&resolved) {
        return Err(format!(
            "output directory must not contain the source directory: {} contains {}",
            dist.display(),
            src.display()
        ));
    }

    Ok(())
}

// Resolve a raw reference to a file under `src`.
pub fn asset_ref_path_resolve(
    src: &Path,
    page_path: &Path,
    asset_ref: &str,
) -> Result<Option<PathBuf>, String> {
    let ref_trimmed = asset_ref.trim();
    if ref_trimmed.is_empty() {
        return Ok(None);
    }

    let (ref_cleaned, _) = url_ref_split(ref_trimmed);
    if is_remote_or_absolute(ref_cleaned) {
        return Ok(None);
    }

    let ref_path = page_path
        .parent()
        .map_or_else(|| src.to_path_buf(), |dir| dir.join(ref_cleaned));
    let ref_path_normalized = path_normalize(&ref_path);
    if !ref_path_normalized.starts_with(src) {
        return Err(format!(
            "{asset_ref} (referenced from {}) escapes {}",
            page_path.display(),
            src.display()
        ));
    }

    let ref_path_canonicalized = fs::canonicalize(&ref_path).map_err(|_| {
        format!(
            "asset not found: {asset_ref} (referenced from {})",
            page_path.display()
        )
    })?;
    if !ref_path_canonicalized.starts_with(src) {
        return Err(format!(
            "{asset_ref} (referenced from {}) escapes {}",
            page_path.display(),
            src.display()
        ));
    }

    Ok(Some(ref_path_canonicalized))
}

// Replace only the filename of a reference with the bundled output name,
// keeping the relative directory prefix and any query/fragment
// (`./scripts/main.js?v=2` → `./scripts/main-<hash>.js?v=2`).
pub fn asset_ref_rewrite(asset_ref: &str, file_name: &str) -> String {
    let (asset_ref_prefix, asset_ref_suffix) = url_ref_split(asset_ref);
    let asset_ref_rewritten = match asset_ref_prefix.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => format!("{dir}/{file_name}"),
        _ => file_name.to_string(),
    };

    format!("{asset_ref_rewritten}{asset_ref_suffix}")
}

// Convert `path` to an absolute path
// - canonicalize the deepest existing ancestor (so symlinks and `..` are resolved)
// - append the not-yet-existing path components
fn dist_path_resolve(path: &Path) -> Result<PathBuf, String> {
    let path_resolved = match path.canonicalize() {
        // Dist path exists on disk
        Ok(resolved) => resolved,
        // Dist path does not exist on disk
        Err(_) => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|err| format!("cannot resolve current directory: {err}"))?
                    .join(path)
            };

            // An array of the yet-non-existing path components
            let missing: Vec<std::ffi::OsString> = absolute
                .ancestors()
                .take_while(|p| !p.exists())
                .filter_map(|p| p.file_name())
                .map(|n| n.to_os_string())
                .collect();

            // The deepest existing path ancestor
            let mut resolved = absolute
                .ancestors()
                .find(|p| p.exists())
                .ok_or_else(|| format!("cannot resolve {}: no existing ancestor", path.display()))?
                .canonicalize()
                .map_err(|err| format!("cannot resolve {}: {err}", path.display()))?;

            for part in missing.into_iter().rev() {
                resolved.push(part);
            }

            // Clean the path
            path_normalize(&resolved)
        }
    };

    Ok(path_resolved)
}

// Clean a path: resolve `.` and `..`.
fn path_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }

    out
}

// Check if the value is a remote or absolute references.
// `http:`, `//`, `data:`, `mailto:`, `/path`
pub fn is_remote_or_absolute(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }
    if let Some((scheme, _)) = value.split_once(':') {
        let mut chars = scheme.chars();
        return chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    false
}

// Split a url reference into its path and any `?query`/`#fragment` suffix.
pub fn url_ref_split(url_ref: &str) -> (&str, &str) {
    match url_ref.find(['?', '#']) {
        Some(idx) => (&url_ref[..idx], &url_ref[idx..]),
        None => (url_ref, ""),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn url_ref_split_handles_query() {
        assert_eq!(url_ref_split("./scripts.js?v=2"), ("./scripts.js", "?v=2"));
    }

    #[test]
    fn url_ref_split_handles_fragment() {
        assert_eq!(url_ref_split("main.css#top"), ("main.css", "#top"));
    }

    #[test]
    fn url_ref_split_query_then_fragment() {
        assert_eq!(url_ref_split("a.js?x=1#f"), ("a.js", "?x=1#f"));
    }

    #[test]
    fn url_ref_split_no_suffix() {
        assert_eq!(url_ref_split("plain.css"), ("plain.css", ""));
    }

    #[test]
    fn url_ref_split_empty() {
        assert_eq!(url_ref_split(""), ("", ""));
    }

    #[test]
    fn is_remote_or_absolute_detects_remotes() {
        for remote in [
            "//cdn.example.com/x.js",
            "http://example.com/x.js",
            "https://x",
            "data:text/css,...",
            "mailto:a@b",
            "/assets/main.css",
        ] {
            assert!(is_remote_or_absolute(remote), "expected remote: {remote}");
        }
    }

    #[test]
    fn is_remote_or_absolute_allows_local() {
        for local in ["./styles.css", "styles.css", "a/b.css", "#anchor", "http"] {
            assert!(!is_remote_or_absolute(local), "expected local: {local}");
        }
    }

    #[test]
    fn path_normalize_resolves_dotdot() {
        assert_eq!(
            path_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
    }

    #[test]
    fn path_normalize_keeps_dot_dirs() {
        assert_eq!(
            path_normalize(Path::new("a/./b/./c")),
            PathBuf::from("a/b/c")
        );
    }

    #[test]
    fn path_normalize_trailing_parent() {
        assert_eq!(path_normalize(Path::new("a/b/..")), PathBuf::from("a"));
    }

    #[test]
    fn asset_ref_rewrite_keeps_dir_and_query() {
        assert_eq!(
            asset_ref_rewrite("./scripts/main.js?v=2", "main-abc.js"),
            "./scripts/main-abc.js?v=2"
        );
    }

    #[test]
    fn asset_ref_rewrite_keeps_dir() {
        assert_eq!(
            asset_ref_rewrite("./scripts/main.js", "main-abc.js"),
            "./scripts/main-abc.js"
        );
    }

    #[test]
    fn asset_ref_rewrite_no_dir() {
        assert_eq!(
            asset_ref_rewrite("main.js?v=2", "main-abc.js"),
            "main-abc.js?v=2"
        );
    }

    #[test]
    fn asset_ref_rewrite_absolute_ref() {
        assert_eq!(
            asset_ref_rewrite("/absolute/main.js#seg", "x.js"),
            "/absolute/x.js#seg"
        );
    }

    #[test]
    fn asset_ref_path_resolve_local() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "<html></html>").unwrap();
        fs::create_dir_all(src.join("scripts")).unwrap();
        fs::write(src.join("scripts/main.js"), "console.log(1);").unwrap();

        let resolved = asset_ref_path_resolve(&src, &page, "./scripts/main.js?v=2")
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved,
            fs::canonicalize(src.join("scripts/main.js")).unwrap()
        );
    }

    #[test]
    fn asset_ref_path_resolve_remote_returns_none() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        for remote in [
            "https://example.com/app.js",
            "//cdn/app.js",
            "/assets/app.js",
            "data:text/javascript,x",
        ] {
            assert_eq!(
                asset_ref_path_resolve(&src, &page, remote).unwrap(),
                None,
                "remote: {remote}"
            );
        }
    }

    #[test]
    fn asset_ref_path_resolve_empty_returns_none() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        assert_eq!(asset_ref_path_resolve(&src, &page, "  ").unwrap(), None);
    }

    #[test]
    fn asset_ref_path_resolve_missing_file_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        let err = asset_ref_path_resolve(&src, &page, "./missing.js").unwrap_err();
        assert!(err.contains("asset not found"), "got: {err}");
    }

    #[test]
    fn asset_ref_path_resolve_escape_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("sub/index.html");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(&page, "").unwrap();

        let err = asset_ref_path_resolve(&src, &page, "../../outside.js").unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn asset_ref_path_resolve_symlink_escape_errs() {
        use std::os::unix::fs::symlink;

        let inside = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let src = fs::canonicalize(inside.path()).unwrap();
        let page = src.join("sub/index.html");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(&page, "").unwrap();

        fs::write(outside.path().join("out.js"), "console.log(1);").unwrap();
        symlink(outside.path().join("out.js"), src.join("sub/src-link.js")).unwrap();

        let err = asset_ref_path_resolve(&src, &page, "./src-link.js").unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");
    }

    #[test]
    fn dist_guard_rejects_equal() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();

        let err = dist_guard(&src, &src).unwrap_err();
        assert!(err.contains("must not be the source"), "got: {err}");
    }

    #[test]
    fn dist_guard_rejects_inside() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();

        let err = dist_guard(&src, &src.join("out")).unwrap_err();
        assert!(err.contains("inside"), "got: {err}");
    }

    #[test]
    fn dist_guard_rejects_containing() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();

        let err = dist_guard(&src, &root).unwrap_err();
        assert!(err.contains("must not contain"), "got: {err}");
    }

    #[test]
    fn dist_guard_accepts_sibling() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        let out = root.join("dist");

        assert!(dist_guard(&src, &out).is_ok());

        fs::create_dir_all(&out).unwrap();
        assert!(dist_guard(&src, &out).is_ok());
    }

    #[test]
    fn dist_path_resolve_existing() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let out = root.join("out");
        fs::create_dir_all(&out).unwrap();

        assert_eq!(dist_path_resolve(&out).unwrap(), out);
    }

    #[test]
    fn dist_path_resolve_missing_nested() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let out = root.join("a").join("b").join("c");

        assert_eq!(dist_path_resolve(&out).unwrap(), out);
    }
}
