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
