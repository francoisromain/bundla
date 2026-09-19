use std::{fs, path::Path};

use walkdir::WalkDir;

use super::assets::{CSS_EXTENSIONS, JS_EXTENSIONS};

// mirror every non-`.html` file under `src` that is not a bundled asset,
// preserving the relative layout. dotted files and dotted directories are
// skipped. unreadable source entries are skipped, reported as warnings; a
// failure to write into `dist` is a hard error.
pub fn static_asset_copy(
    src: &Path,
    dist: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    for entry in WalkDir::new(src)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
        })
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                warnings.push(format!(
                    "skipping unreadable path during static copy: {err}"
                ));
                continue;
            }
        };
        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "html"
            || CSS_EXTENSIONS.contains(&ext.as_str())
            || JS_EXTENSIONS.contains(&ext.as_str())
        {
            continue;
        }

        let out = dist.join(
            path.strip_prefix(src)
                .expect("static asset path is always under src"),
        );
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        match fs::metadata(path) {
            Ok(_) => {}
            Err(err) => {
                warnings.push(format!(
                    "skipping unreadable static asset {}: {err}",
                    path.display()
                ));
                continue;
            }
        }
        if let Err(err) = fs::copy(path, &out) {
            // a stat-able source can still be unreadable (e.g. mode 000),
            // so probe readability before blaming the destination
            if fs::read(path).is_err() {
                warnings.push(format!(
                    "skipping unreadable static asset {}: {err}",
                    path.display()
                ));
                continue;
            }
            return Err(format!(
                "failed to copy {} into {}: {err}",
                path.display(),
                out.display()
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write_file(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn list_rel(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                e.path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn static_asset_copy_mirrors_static_files_only() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        write_file(&src, "index.html", "<html></html>");
        write_file(&src, "img/logo.png", "png-bytes");
        write_file(&src, "fonts/x.woff", "woff-bytes");
        write_file(&src, "noext", "no-extension");
        write_file(&src, "scripts/app.js", "app-js");
        write_file(&src, "scripts/nested/main.CSS", "css");
        write_file(&src, "styles/theme.css", "css");
        write_file(&src, "styles/nested/more.mjs", "js");
        write_file(&src, ".gitignore", "ignored");
        write_file(&src, ".git/HEAD", "ref");

        let dist = src.join("out");
        let mut warnings = Vec::new();
        static_asset_copy(&src, &dist, &mut warnings).unwrap();

        assert_eq!(list_rel(&dist), ["fonts/x.woff", "img/logo.png", "noext"]);
        assert_eq!(
            fs::read_to_string(dist.join("img/logo.png")).unwrap(),
            "png-bytes"
        );
        assert_eq!(
            fs::read_to_string(dist.join("fonts/x.woff")).unwrap(),
            "woff-bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn static_asset_copy_skips_unreadable_source_without_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        write_file(&src, "ok.txt", "fine");
        write_file(&src, "secret.txt", "secret");
        fs::set_permissions(src.join("secret.txt"), fs::Permissions::from_mode(0o000)).unwrap();

        let dist = src.join("out");
        let mut warnings = Vec::new();
        static_asset_copy(&src, &dist, &mut warnings).unwrap();

        assert!(dist.join("ok.txt").is_file());
        assert!(
            warnings.iter().any(|w| w.contains("secret.txt")),
            "expected a warning about the unreadable asset, got: {warnings:?}"
        );
    }
}
