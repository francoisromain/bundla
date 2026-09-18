use std::{fs, path::Path};

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use parcel_sourcemap::SourceMap;

use super::{super::Options, dist_mkdir, file_name_output_format, file_stem_extract};

pub fn css_bundle(
    source: &Path,
    dist: &Path,
    dir: &Path,
    options: Options,
) -> Result<String, String> {
    let css_code =
        fs::read_to_string(source).map_err(|err| format!("css source unreadable: {err}"))?;
    let mut stylesheet = StyleSheet::parse(&css_code, ParserOptions::default())
        .map_err(|err| format!("css parse error: {err}"))?;
    if options.minify {
        stylesheet
            .minify(MinifyOptions::default())
            .map_err(|err| format!("css minify error: {err}"))?;
    }

    let file_stem = file_stem_extract(source, "styles");

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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write_css(source: &std::path::Path, content: &str) {
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(source, content).unwrap();
    }

    #[test]
    fn css_parse_error_on_invalid_rules() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("styles/bad.css");
        write_css(&source, "}");

        let err = css_bundle(
            &source,
            &dir.path().join("out"),
            Path::new("styles"),
            Options::DEV,
        )
        .unwrap_err();
        assert!(err.contains("css parse error"), "got: {err}");
    }

    #[test]
    fn css_source_unreadable_errs() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("missing.css");

        let err = css_bundle(
            &source,
            &dir.path().join("out"),
            Path::new(""),
            Options::DEV,
        )
        .unwrap_err();
        assert!(err.contains("css source unreadable"), "got: {err}");
    }

    #[test]
    fn css_dev_emits_map_and_sourcemap_tail() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("styles/theme.css");
        write_css(&source, "body { color: red; }\n");
        let dist = dir.path().join("out");

        let name = css_bundle(&source, &dist, Path::new("styles"), Options::DEV).unwrap();
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
        let source = dir.path().join("styles/theme.css");
        write_css(&source, "body { color: red; }\n");
        let dist = dir.path().join("out");

        let name = css_bundle(&source, &dist, Path::new("styles"), Options::RELEASE).unwrap();
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
}
