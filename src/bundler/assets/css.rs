use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use parcel_sourcemap::SourceMap;

use super::{
    super::{Options, path},
    dist_mkdir, file_name_output_format, file_stem_extract,
};

pub fn css_bundle(
    asset_path: &Path,
    src: &Path,
    dist: &Path,
    dir: &Path,
    options: Options,
    cache: &mut HashMap<PathBuf, String>,
) -> Result<String, String> {
    let mut stack: Vec<PathBuf> = Vec::new();
    css_bundle_rec(asset_path, src, dist, dir, options, cache, &mut stack)
}

// the stack carries the in-progress sources so that a cycle in the `@import`
// graph is detected instead of recursing forever
fn css_bundle_rec(
    asset_path: &Path,
    src: &Path,
    dist: &Path,
    dir: &Path,
    options: Options,
    cache: &mut HashMap<PathBuf, String>,
    stack: &mut Vec<PathBuf>,
) -> Result<String, String> {
    let asset_path =
        fs::canonicalize(asset_path).map_err(|err| format!("css source unreadable: {err}"))?;
    let css_code =
        fs::read_to_string(&asset_path).map_err(|err| format!("css source unreadable: {err}"))?;
    let mut stylesheet = StyleSheet::parse(&css_code, ParserOptions::default())
        .map_err(|err| format!("css parse error: {err}"))?;

    stack.push(asset_path.clone());
    let err = css_imports_rewrite(
        &mut stylesheet,
        &asset_path,
        src,
        dist,
        options,
        cache,
        stack,
    );
    stack.pop();
    err?;

    if options.minify {
        stylesheet
            .minify(MinifyOptions::default())
            .map_err(|err| format!("css minify error: {err}"))?;
    }

    let file_stem = file_stem_extract(&asset_path, "styles");

    let mut map = SourceMap::new("/");
    let result = stylesheet
        .to_css(PrinterOptions {
            minify: options.minify,
            source_map: options.sourcemap.then_some(&mut map),
            ..PrinterOptions::default()
        })
        .map_err(|err| format!("css print error: {err}"))?;

    let mut code = result.code;
    let out = dist_mkdir(dist, dir)?;
    if options.sourcemap {
        let map_json = map
            .to_json(None)
            .map_err(|err| format!("css sourcemap serialization error: {err}"))?;
        let map_name = format!("{file_stem}.map");
        fs::write(out.join(&map_name), map_json)
            .map_err(|err| format!("css sourcemap write error: {err}"))?;
        code.push_str(&format!("\n/*# sourceMappingURL={map_name} */"));
    }

    let name = file_name_output_format(&file_stem, "css", code.as_bytes(), options);
    fs::write(out.join(&name), code).map_err(|err| format!("css output write error: {err}"))?;
    Ok(name)
}

// bundle every relative `@import` target and rewrite the rule's url to its
// output name; remotes and absolute-rooted refs are left untouched for the
// browser to resolve, and missing local targets are hard errors. import
// targets are always content-hashed, dev included, so editing an imported
// file yields a new url and the browser re-fetches it instead of serving a
// cached copy of the parent css.
fn css_imports_rewrite(
    stylesheet: &mut StyleSheet,
    asset_path: &Path,
    src: &Path,
    dist: &Path,
    options: Options,
    cache: &mut HashMap<PathBuf, String>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for import in stylesheet.rules.0.iter_mut().filter_map(|rule| match rule {
        CssRule::Import(import) => Some(import),
        _ => None,
    }) {
        let import_ref = import.url.as_ref();
        let (import_ref_cleaned, _) = path::url_ref_split(import_ref);
        if import_ref_cleaned.trim().is_empty() || path::is_remote_or_absolute(import_ref_cleaned) {
            continue;
        }

        let Some(import_path) = path::asset_ref_path_resolve(src, asset_path, import_ref_cleaned)?
        else {
            continue;
        };
        if stack.contains(&import_path) {
            return Err(format!(
                "css import cycle: '{}' imports {}",
                asset_path.display(),
                import_path.display()
            ));
        }

        let file_name = match cache.get(&import_path) {
            Some(file_name) => file_name.clone(),
            None => {
                let imported_dir = import_path
                    .strip_prefix(src)
                    .map_err(|_| {
                        format!(
                            "css import escapes source: {} (imported from {})",
                            import_path.display(),
                            asset_path.display()
                        )
                    })?
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
                let file_name = css_bundle_rec(
                    &import_path,
                    src,
                    dist,
                    &imported_dir,
                    Options {
                        hash: true,
                        ..options
                    },
                    cache,
                    stack,
                )?;
                cache.insert(import_path.clone(), file_name.clone());
                file_name
            }
        };

        let rewritten = path::asset_ref_rewrite(import_ref_cleaned, &file_name);
        import.url = rewritten.into();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write_css(source: &Path, content: &str) {
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(source, content).unwrap();
    }

    fn bundle_css(
        source: &Path,
        src: &Path,
        dist: &Path,
        dir: &Path,
        options: Options,
    ) -> Result<String, String> {
        css_bundle(source, src, dist, dir, options, &mut HashMap::new())
    }

    fn list_names(dir: &Path) -> Vec<String> {
        if !dir.exists() {
            return Vec::new();
        }
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn css_parse_error_on_invalid_rules() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("styles/bad.css");
        write_css(&source, "}");

        let err = bundle_css(
            &source,
            &src,
            &src.join("out"),
            Path::new("styles"),
            Options::DEV,
        )
        .unwrap_err();
        assert!(err.contains("css parse error"), "got: {err}");
    }

    #[test]
    fn css_source_unreadable_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("missing.css");

        let err =
            bundle_css(&source, &src, &src.join("out"), Path::new(""), Options::DEV).unwrap_err();
        assert!(err.contains("css source unreadable"), "got: {err}");
    }

    #[test]
    fn css_dev_emits_map_and_sourcemap_tail() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("styles/theme.css");
        write_css(&source, "body { color: red; }\n");
        let dist = src.join("out");

        let name = bundle_css(&source, &src, &dist, Path::new("styles"), Options::DEV).unwrap();
        assert_eq!(name, "theme.css");
        let code = fs::read_to_string(dist.join("styles/theme.css")).unwrap();
        assert!(
            code.ends_with("/*# sourceMappingURL=theme.map */"),
            "got: {code}"
        );
        assert!(dist.join("styles/theme.map").is_file());
    }

    #[test]
    fn css_release_minifies_and_hashes_without_map() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        let source = src.join("styles/theme.css");
        write_css(&source, "body { color: red; }\n");
        let dist = src.join("out");

        let name = bundle_css(&source, &src, &dist, Path::new("styles"), Options::RELEASE).unwrap();
        assert!(
            name.starts_with("theme-") && name.ends_with(".css"),
            "got: {name}"
        );
        assert!(!dist.join("styles/theme.map").exists());
        let code = fs::read_to_string(dist.join("styles").join(&name)).unwrap();
        assert!(code.contains("color"), "got: {code}");
        assert!(
            !code.contains(char::is_whitespace),
            "expected minified output, got: {code}"
        );
    }

    #[test]
    fn css_import_rewritten_in_release() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(src.join("styles/sub")).unwrap();
        write_css(
            &src.join("styles/main.css"),
            "@import url(\"https://cdn.example.com/remote.css\");\n@import url(\"./sub/reset.css\");\nbody { color: red; }\n",
        );
        write_css(&src.join("styles/sub/reset.css"), "p { margin: 0; }\n");
        let dist = src.join("out");

        bundle_css(
            &src.join("styles/main.css"),
            &src,
            &dist,
            Path::new("styles"),
            Options::RELEASE,
        )
        .unwrap();

        let resets = list_names(&dist.join("styles/sub"));
        assert_eq!(resets.len(), 1, "expected one reset output: {resets:?}");
        let reset_name = &resets[0];
        assert!(
            reset_name.starts_with("reset-") && reset_name.ends_with(".css"),
            "{reset_name}"
        );
        assert!(
            dist.join("styles/sub").join(reset_name).is_file(),
            "missing {}",
            dist.join("styles/sub").join(reset_name).display()
        );

        let mains = list_names(&dist.join("styles"));
        assert_eq!(mains.len(), 1, "expected one main output: {mains:?}");
        let out_code = fs::read_to_string(dist.join("styles").join(&mains[0])).unwrap();
        assert!(out_code.contains(reset_name), "got: {out_code}");
        assert!(
            out_code.contains("cdn.example.com/remote.css"),
            "remote import must be left as-is, got: {out_code}"
        );
    }

    #[test]
    fn css_import_preserves_media_query() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(src.join("styles/sub")).unwrap();
        write_css(
            &src.join("styles/main.css"),
            "@import url(\"./sub/reset.css\") (min-width: 100px);\nbody { color: red; }\n",
        );
        write_css(&src.join("styles/sub/reset.css"), "p { margin: 0; }\n");
        let dist = src.join("out");

        bundle_css(
            &src.join("styles/main.css"),
            &src,
            &dist,
            Path::new("styles"),
            Options::DEV,
        )
        .unwrap();

        let resets: Vec<String> = list_names(&dist.join("styles/sub"))
            .into_iter()
            .filter(|n| n.ends_with(".css"))
            .collect();
        assert_eq!(resets.len(), 1, "{resets:?}");
        assert!(
            resets[0].starts_with("reset-") && resets[0].ends_with(".css"),
            "dev import target must be content-hashed: {}",
            resets[0]
        );
        let out_code = fs::read_to_string(dist.join("styles/main.css")).unwrap();
        assert!(out_code.contains(&resets[0]), "got: {out_code}");
        assert!(
            out_code.contains("width"),
            "media condition kept, got: {out_code}"
        );
        assert!(
            out_code.contains("100px"),
            "media condition kept, got: {out_code}"
        );
        let reset_code = fs::read_to_string(dist.join("styles/sub").join(&resets[0])).unwrap();
        assert!(reset_code.contains("margin"), "got: {reset_code}");
    }

    #[test]
    fn css_import_dev_hashes_import_targets() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(src.join("styles/sub")).unwrap();
        let main_source = src.join("styles/main.css");
        let reset_source = src.join("styles/sub/reset.css");
        write_css(&main_source, "@import url(\"./sub/reset.css\");\nbody{}\n");
        let first_dist = src.join("out1");

        write_css(&reset_source, "p { margin: 0; }\n");
        bundle_css(
            &main_source,
            &src,
            &first_dist,
            Path::new("styles"),
            Options::DEV,
        )
        .unwrap();
        let first_resets: Vec<String> = list_names(&first_dist.join("styles/sub"))
            .into_iter()
            .filter(|n| n.ends_with(".css"))
            .collect();
        assert_eq!(first_resets.len(), 1, "{first_resets:?}");
        assert!(
            first_resets[0].starts_with("reset-") && first_resets[0].ends_with(".css"),
            "dev import target must be content-hashed: {}",
            first_resets[0]
        );
        let first_main = fs::read_to_string(first_dist.join("styles/main.css")).unwrap();
        assert!(first_main.contains(&first_resets[0]), "got: {first_main}");

        let second_dist = src.join("out2");
        write_css(&reset_source, "p { margin: 0 0 1rem; }\n");
        bundle_css(
            &main_source,
            &src,
            &second_dist,
            Path::new("styles"),
            Options::DEV,
        )
        .unwrap();
        let second_resets: Vec<String> = list_names(&second_dist.join("styles/sub"))
            .into_iter()
            .filter(|n| n.ends_with(".css"))
            .collect();
        assert_eq!(second_resets.len(), 1, "{second_resets:?}");
        assert!(
            second_resets[0].starts_with("reset-") && second_resets[0].ends_with(".css"),
            "got: {}",
            second_resets[0]
        );
        assert_ne!(
            second_resets[0], first_resets[0],
            "editing the imported file must change its output name so browsers re-fetch it"
        );
        let second_main = fs::read_to_string(second_dist.join("styles/main.css")).unwrap();
        assert!(
            second_main.contains(&second_resets[0]),
            "got: {second_main}"
        );
    }

    #[test]
    fn css_import_dedup_across_parents() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(src.join("styles/shared")).unwrap();
        write_css(
            &src.join("styles/main.css"),
            "@import url(\"./shared/reset.css\");\nbody{}\n",
        );
        write_css(
            &src.join("styles/theme.css"),
            "@import url(\"./shared/reset.css\");\np{}\n",
        );
        write_css(&src.join("styles/shared/reset.css"), "a{}\n");
        let dist = src.join("out");

        let mut cache = HashMap::new();
        css_bundle(
            &src.join("styles/main.css"),
            &src,
            &dist,
            Path::new("styles"),
            Options::RELEASE,
            &mut cache,
        )
        .unwrap();
        css_bundle(
            &src.join("styles/theme.css"),
            &src,
            &dist,
            Path::new("styles"),
            Options::RELEASE,
            &mut cache,
        )
        .unwrap();

        let resets = list_names(&dist.join("styles/shared"));
        assert_eq!(
            resets.len(),
            1,
            "expected the shared import bundled once: {resets:?}"
        );

        let mains = list_names(&dist.join("styles"));
        assert_eq!(mains.len(), 2, "expected both parents' outputs: {mains:?}");
        for main_name in &mains {
            let main_code = fs::read_to_string(dist.join("styles").join(main_name)).unwrap();
            assert!(
                main_code.contains(&resets[0]),
                "{main_name} must reference {}: {main_code}",
                resets[0]
            );
        }
    }

    #[test]
    fn css_import_cycle_detected() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(src.join("sub")).unwrap();
        write_css(
            &src.join("a.css"),
            "@import url(\"./sub/b.css\");\nbody{}\n",
        );
        write_css(&src.join("sub/b.css"), "@import url(\"../a.css\");\np{}\n");
        let dist = src.join("out");

        let err = bundle_css(
            &src.join("a.css"),
            &src,
            &dist,
            Path::new(""),
            Options::RELEASE,
        )
        .unwrap_err();
        assert!(err.contains("css import cycle"), "got: {err}");
    }

    #[test]
    fn css_import_missing_target_errs() {
        let dir = tempdir().unwrap();
        let src = fs::canonicalize(dir.path()).unwrap();
        write_css(
            &src.join("styles/main.css"),
            "@import url(\"./nope.css\");\nbody{}\n",
        );
        let dist = src.join("out");

        let err = bundle_css(
            &src.join("styles/main.css"),
            &src,
            &dist,
            Path::new("styles"),
            Options::RELEASE,
        )
        .unwrap_err();
        assert!(err.contains("asset not found"), "got: {err}");
    }
}
