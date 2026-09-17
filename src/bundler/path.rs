use std::{
    fs,
    path::{Component, Path, PathBuf},
};

// reject `dist` if equal to, inside, or containing `src`
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

// resolve a raw reference to a file under `src`
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

// replace only the filename of a reference with the bundled output name,
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

// convert `path` to an absolute path
// - canonicalize the deepest existing ancestor (so symlinks and `..` are resolved)
// - append the not-yet-existing path components
fn dist_path_resolve(path: &Path) -> Result<PathBuf, String> {
    let path_resolved = match path.canonicalize() {
        // dist path exists on disk
        Ok(resolved) => resolved,
        // dist path does not exist on disk
        Err(_) => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|err| format!("cannot resolve current directory: {err}"))?
                    .join(path)
            };

            // an array of the yet-non-existing path components
            let missing: Vec<std::ffi::OsString> = absolute
                .ancestors()
                .take_while(|p| !p.exists())
                .filter_map(|p| p.file_name())
                .map(|n| n.to_os_string())
                .collect();

            // the deepest existing path ancestor
            let mut resolved = absolute
                .ancestors()
                .find(|p| p.exists())
                .ok_or_else(|| format!("cannot resolve {}: no existing ancestor", path.display()))?
                .canonicalize()
                .map_err(|err| format!("cannot resolve {}: {err}", path.display()))?;

            for part in missing.into_iter().rev() {
                resolved.push(part);
            }

            // clean the path
            path_normalize(&resolved)
        }
    };

    Ok(path_resolved)
}

// clean a path: resolve `.` and `..`
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

// check if the value is a remote or absolute references
// `http:`, `//`, `data:`, `mailto:`, `/path`
fn is_remote_or_absolute(value: &str) -> bool {
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

// split a url reference into its path and any `?query`/`#fragment` suffix
fn url_ref_split(url_ref: &str) -> (&str, &str) {
    match url_ref.find(['?', '#']) {
        Some(idx) => (&url_ref[..idx], &url_ref[idx..]),
        None => (url_ref, ""),
    }
}
