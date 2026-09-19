use std::{fs, path::Path};

use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, RawMinifyOptions, SourceMapType};

use super::{super::Options, dist_mkdir, file_name_output_format, file_stem_extract};

pub async fn js_bundle(
    source: &Path,
    src: &Path,
    rel: &Path,
    dist: &Path,
    dir: &Path,
    options: Options,
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    let stem = file_stem_extract(source, "scripts");
    let import = rel
        .to_str()
        .ok_or_else(|| format!("path is not valid utf-8: {}", rel.display()))?;

    let mut bundler = Bundler::new(BundlerOptions {
        input: Some(vec![InputItem {
            name: Some(stem.clone()),
            import: import.to_string(),
        }]),
        cwd: Some(src.to_path_buf()),
        format: Some(OutputFormat::Esm),
        minify: Some(RawMinifyOptions::Bool(options.minify)),
        sourcemap: options.sourcemap.then_some(SourceMapType::File),
        ..BundlerOptions::default()
    })
    .map_err(|err| format!("bundler init error: {err}"))?;

    let output = bundler
        .generate()
        .await
        .map_err(|err| format!("js bundle error: {err}"))?;

    for warning in output.warnings {
        warnings.push(format!("js bundle: {warning}"));
    }

    let dist_dir = dist_mkdir(dist, dir)?;
    // Rolldown guarantees asset sorting:
    // - entry chunks first,
    // - then secondary chunks,
    // - then assets (sourcemaps)
    // (see `finalize_assets` in rolldown's `stages/generate_stage`)
    // So the first non-`.map` asset below is the entry chunk,
    // and renaming only it keeps the entry's output name predictable.
    // secondary chunks keep their rolldown-generated `[name]-[hash].js` names.
    // With hashed filenames the entry's sourcemap is renamed to match
    // (`<final_name>.map`) and the trailing `sourceMappingURL` comment is
    // patched to the new name, keeping the map paired with the hashed entry.
    let mut js_name = None;
    let mut entry_orig = None;
    for item in &output.assets {
        let name = item.filename();
        if name.ends_with(".map") {
            let map_name = Path::new(name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(name);
            // In hashed mode only the entry's own map is renamed to match the
            // entry; secondary chunk maps keep their rolldown names and their
            // chunks' `sourceMappingURL` comments stay valid untouched.
            let is_entry_map = options.hash
                && js_name.is_some()
                && entry_orig
                    .as_deref()
                    .is_some_and(|orig| map_name == format!("{orig}.map"));
            if is_entry_map {
                fs::write(
                    dist_dir.join(format!("{}.map", js_name.as_deref().unwrap())),
                    item.content_as_bytes(),
                )
                .map_err(|err| format!("js sourcemap write error: {err}"))?;
                continue;
            }
            fs::write(dist_dir.join(map_name), item.content_as_bytes())
                .map_err(|err| format!("js sourcemap write error: {err}"))?;

            continue;
        }

        let final_name =
            file_name_output_format(&stem, "js", item.content_as_bytes(), options.hash);
        if js_name.is_none() {
            let bytes = if options.sourcemap {
                let map_name = format!("{name}.map");
                let final_map = format!("{final_name}.map");
                sourcemap_comment_rewrite(
                    &String::from_utf8_lossy(item.content_as_bytes()),
                    &map_name,
                    &final_map,
                )
                .into_bytes()
            } else {
                item.content_as_bytes().to_vec()
            };
            fs::write(dist_dir.join(&final_name), bytes)
                .map_err(|err| format!("js output write error: {err}"))?;
            entry_orig = Some(name.to_string());
            js_name = Some(final_name);
        } else {
            fs::write(dist_dir.join(name), item.content_as_bytes())
                .map_err(|err| format!("js chunk write error: {err}"))?;
        }
    }

    js_name.ok_or_else(|| format!("no js output for {}", source.display()))
}

// Rewrite the last trailing `sourceMappingURL={from}` comment to
// `sourceMappingURL={to}`, so a hashed entry keeps pointing at its map
// after the map is renamed to match. no-op when `from` is absent or equal.
fn sourcemap_comment_rewrite(text: &str, from: &str, to: &str) -> String {
    if from == to {
        return text.to_string();
    }
    let needle = format!("sourceMappingURL={from}");
    let Some(pos) = text.rfind(&needle) else {
        return text.to_string();
    };
    let mut out = text[..pos].to_string();
    out.push_str(&format!("sourceMappingURL={to}"));
    out.push_str(&text[pos + needle.len()..]);
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn sourcemap_comment_rewrite_replaces_last_occurrence() {
        let text = "export{}\n//# sourceMappingURL=app.js.map";
        let out = sourcemap_comment_rewrite(text, "app.js.map", "app-deadbeef.js.map");
        assert_eq!(out, "export{}\n//# sourceMappingURL=app-deadbeef.js.map");
    }

    #[test]
    fn sourcemap_comment_rewrite_keeps_earlier_occurrences() {
        let text = "//# sourceMappingURL=app.js.map\nfoo()\n//# sourceMappingURL=app.js.map";
        assert_eq!(
            sourcemap_comment_rewrite(text, "app.js.map", "app-x.js.map"),
            "//# sourceMappingURL=app.js.map\nfoo()\n//# sourceMappingURL=app-x.js.map"
        );
    }

    #[test]
    fn sourcemap_comment_rewrite_noop_when_equal_or_absent() {
        assert_eq!(
            sourcemap_comment_rewrite(
                "a\n//# sourceMappingURL=app.js.map",
                "app.js.map",
                "app.js.map"
            ),
            "a\n//# sourceMappingURL=app.js.map"
        );
        assert_eq!(
            sourcemap_comment_rewrite("console.log(1);", "app.js.map", "app-x.js.map"),
            "console.log(1);"
        );
    }

    fn write_js(source: &std::path::Path, content: &str) {
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(source, content).unwrap();
    }

    #[tokio::test]
    async fn js_syntax_error_returns_bundle_error() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("scripts/app.js");
        write_js(&source, "const = ;\n");

        let err = js_bundle(
            &source,
            &src,
            Path::new("scripts/app.js"),
            &src.join("out"),
            Path::new("scripts"),
            Options::DEV,
            &mut vec![],
        )
        .await
        .unwrap_err();
        assert!(err.contains("js bundle error"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn js_non_utf8_rel_path_errs() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let rel = std::path::Path::new(std::ffi::OsStr::from_bytes(&[0xff, b'.', b'j', b's']));
        fs::write(src.join(rel), "console.log(1);\n").unwrap();

        let err = js_bundle(
            &fs::canonicalize(src.join(rel)).unwrap(),
            &src,
            rel,
            &src.join("out"),
            Path::new(""),
            Options::DEV,
            &mut vec![],
        )
        .await
        .unwrap_err();
        assert!(err.contains("not valid utf-8"), "got: {err}");
    }

    #[tokio::test]
    async fn js_release_hashes_without_map() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("scripts/app.js");
        write_js(&source, "console.log('app');\n");
        let dist = src.join("out");

        let name = js_bundle(
            &source,
            &src,
            Path::new("scripts/app.js"),
            &dist,
            Path::new("scripts"),
            Options::RELEASE,
            &mut vec![],
        )
        .await
        .unwrap();
        assert!(
            name.starts_with("app-") && name.ends_with(".js"),
            "got: {name}"
        );
        assert!(dist.join("scripts").join(&name).is_file());
        let maps: Vec<_> = fs::read_dir(dist.join("scripts"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".map"))
            .collect();
        assert!(maps.is_empty(), "expected no sourcemaps, got: {maps:?}");
    }
}
