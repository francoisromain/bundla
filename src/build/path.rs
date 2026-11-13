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

// Resolve a raw reference to a file under `src`:
// - relative references resolve against the page directory,
// - root-relative references (`/path`) resolve against the `src` root,
//   but only when they start with `base` (the URL prefix the output is
//   served under); without a base, or outside of it, they are left for
//   the browser and `None` is returned.
pub fn asset_ref_path_resolve(
    src: &Path,
    page_path: &Path,
    asset_ref: &str,
    base: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let ref_trimmed = asset_ref.trim();
    if ref_trimmed.is_empty() {
        return Ok(None);
    }

    let (ref_cleaned, _) = url_ref_split(ref_trimmed);
    if is_remote(ref_cleaned) {
        return Ok(None);
    }

    let ref_path = if is_root_relative(ref_cleaned) {
        match base.and_then(|base| base_ref_strip(ref_cleaned, base)) {
            Some(ref_src) => src.join(ref_src),
            // root-relative outside the base, or no base at all
            None => return Ok(None),
        }
    } else {
        page_path
            .parent()
            .map_or_else(|| src.to_path_buf(), |dir| dir.join(ref_cleaned))
    };
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
    if !ref_path_canonicalized.is_file() {
        return Err(format!(
            "asset is not a file: {asset_ref} (referenced from {})",
            page_path.display()
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
        Some((dir, _)) => format!("{dir}/{file_name}"),
        None => file_name.to_string(),
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

// Strip `base` from a root-relative reference, giving the source path
// under `src`. `None` when the reference is outside the base.
// A base of `/` (the site root) contains every root-relative reference;
// a trailing `/` on the base is ignored (`/app/` == `/app`).
fn base_ref_strip<'a>(ref_cleaned: &'a str, base: &str) -> Option<&'a str> {
    let base = base.trim_end_matches('/');
    let stripped = ref_cleaned.strip_prefix(base)?;
    // "" is the site root; otherwise the base must be a whole path
    // segment: `/app` contains `/app/x.css` but not `/application.css`,
    // and `/app` alone resolves to the `src` root itself (a directory)
    if !base.is_empty() && !stripped.is_empty() && !stripped.starts_with('/') {
        return None;
    }

    Some(stripped.strip_prefix('/').unwrap_or(stripped))
}

// Warning for a root-relative reference left for the browser because no
// base is set: bundling it requires one.
pub fn asset_ref_skip_warn(asset_ref: &str, owner: &Path) -> String {
    format!(
        "skipping root-relative reference (set a base to bundle it): {asset_ref} in {}",
        owner.display()
    )
}

// Check if the value is a remote reference:
// - protocol-relative (`//host/path`)
// - absolute URI with any scheme (`https:`, `data:`, `mailto:`, …)
pub fn is_remote(value: &str) -> bool {
    if value.starts_with("//") {
        return true;
    }
    if let Some((scheme, _)) = value.split_once(':') {
        let mut chars = scheme.chars();
        return chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    false
}

// Check if the value is a root-relative reference (`/path`):
// starts with `/` but is not remote.
pub fn is_root_relative(value: &str) -> bool {
    value.starts_with('/') && !is_remote(value)
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
    fn is_remote_detects_remotes() {
        for remote in [
            "//cdn.example.com/x.js",
            "http://example.com/x.js",
            "https://x",
            "data:text/css,...",
            "mailto:a@b",
        ] {
            assert!(is_remote(remote), "expected remote: {remote}");
        }
    }

    #[test]
    fn is_remote_allows_local() {
        for local in [
            "./styles.css",
            "styles.css",
            "a/b.css",
            "#anchor",
            "http",
            "/assets/main.css",
        ] {
            assert!(!is_remote(local), "expected local: {local}");
        }
    }

    #[test]
    fn is_root_relative_detects_paths() {
        for value in ["/assets/main.css", "/app", "/", "/a:b.css"] {
            assert!(is_root_relative(value), "expected root-relative: {value}");
        }
    }

    #[test]
    fn is_root_relative_excludes_remotes_and_relative() {
        for value in [
            "./styles.css",
            "styles.css",
            "//cdn.example.com/x.js",
            "https://x",
            "",
        ] {
            assert!(
                !is_root_relative(value),
                "unexpected root-relative: {value}"
            );
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
    fn asset_ref_rewrite_root_level() {
        assert_eq!(
            asset_ref_rewrite("/main.js?v=1#top", "main-abc.js"),
            "/main-abc.js?v=1#top"
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

        let resolved = asset_ref_path_resolve(&src, &page, "./scripts/main.js?v=2", None)
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
            "data:text/javascript,x",
        ] {
            assert_eq!(
                asset_ref_path_resolve(&src, &page, remote, None).unwrap(),
                None,
                "remote: {remote}"
            );
        }
    }

    #[test]
    fn asset_ref_path_resolve_root_relative_without_base_returns_none() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();
        fs::create_dir_all(src.join("assets")).unwrap();
        fs::write(src.join("assets/app.js"), "console.log(1);").unwrap();

        // left for the browser: the file exists but no base maps the ref
        assert_eq!(
            asset_ref_path_resolve(&src, &page, "/assets/app.js", None).unwrap(),
            None
        );
    }

    #[test]
    fn asset_ref_path_resolve_empty_returns_none() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        assert_eq!(
            asset_ref_path_resolve(&src, &page, "  ", None).unwrap(),
            None
        );
    }

    #[test]
    fn asset_ref_path_resolve_missing_file_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        let err = asset_ref_path_resolve(&src, &page, "./missing.js", None).unwrap_err();
        assert!(err.contains("asset not found"), "got: {err}");
    }

    #[test]
    fn asset_ref_path_resolve_escape_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("sub/index.html");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(&page, "").unwrap();

        let err = asset_ref_path_resolve(&src, &page, "../../outside.js", None).unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");
    }

    #[test]
    fn asset_ref_path_resolve_directory_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();
        fs::create_dir_all(src.join("styles")).unwrap();

        let err = asset_ref_path_resolve(&src, &page, "./styles", None).unwrap_err();
        assert!(err.contains("not a file"), "got: {err}");
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

        let err = asset_ref_path_resolve(&src, &page, "./src-link.js", None).unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");
    }

    #[test]
    fn asset_ref_path_resolve_base_root_resolves_against_src() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("sub/index.html");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(&page, "").unwrap();
        fs::create_dir_all(src.join("scripts")).unwrap();
        fs::write(src.join("scripts/main.js"), "console.log(1);").unwrap();

        let resolved = asset_ref_path_resolve(&src, &page, "/scripts/main.js", Some("/"))
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved,
            fs::canonicalize(src.join("scripts/main.js")).unwrap()
        );
    }

    #[test]
    fn asset_ref_path_resolve_base_prefix_resolves_against_src() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("sub/index.html");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(&page, "").unwrap();
        fs::create_dir_all(src.join("scripts")).unwrap();
        fs::write(src.join("scripts/main.js"), "console.log(1);").unwrap();

        // the page sits in sub/, but a base ref resolves against src root
        let resolved =
            asset_ref_path_resolve(&src, &page, "/app/scripts/main.js?v=2", Some("/app"))
                .unwrap()
                .unwrap();
        assert_eq!(
            resolved,
            fs::canonicalize(src.join("scripts/main.js")).unwrap()
        );
    }

    #[test]
    fn asset_ref_path_resolve_base_trailing_slash_matches() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();
        fs::write(src.join("main.css"), "body{}\n").unwrap();

        let resolved = asset_ref_path_resolve(&src, &page, "/app/main.css", Some("/app/"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved, fs::canonicalize(src.join("main.css")).unwrap());
    }

    #[test]
    fn asset_ref_path_resolve_base_mismatch_returns_none() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();
        fs::write(src.join("main.css"), "body{}\n").unwrap();

        // not under the base: left for the browser
        assert_eq!(
            asset_ref_path_resolve(&src, &page, "/styles/main.css", Some("/app")).unwrap(),
            None
        );
        // `/app` is a whole segment: `/application.css` is not under it
        assert_eq!(
            asset_ref_path_resolve(&src, &page, "/application.css", Some("/app")).unwrap(),
            None
        );
    }

    #[test]
    fn asset_ref_path_resolve_base_escape_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        let err = asset_ref_path_resolve(&src, &page, "/../outside.js", Some("/")).unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");

        let err =
            asset_ref_path_resolve(&src, &page, "/app/../outside.js", Some("/app")).unwrap_err();
        assert!(err.contains("escapes"), "got: {err}");
    }

    #[test]
    fn asset_ref_path_resolve_base_ref_equals_base_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let page = src.join("index.html");
        fs::write(&page, "").unwrap();

        // the ref is the base itself, i.e. the src root directory
        let err = asset_ref_path_resolve(&src, &page, "/app", Some("/app")).unwrap_err();
        assert!(err.contains("not a file"), "got: {err}");
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
