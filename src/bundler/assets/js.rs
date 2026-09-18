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
        eprintln!("js bundle warning: {warning}");
    }

    let dist_dir = dist_mkdir(dist, dir)?;
    // rolldown guarantees asset sorting: entry chunks first,
    // then secondary chunks, then assets (sourcemaps)
    // (see `finalize_assets` in rolldown's `stages/generate_stage`)
    // So the first non-`.map` asset below is the entry chunk,
    // and renaming only it keeps the entry's output name predictable.
    // secondary chunks keep their rolldown-generated `[name]-[hash].js` names.
    let mut js_name = None;
    for item in &output.assets {
        let name = item.filename();
        if name.ends_with(".map") {
            let map_name = Path::new(name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(name);
            fs::write(dist_dir.join(map_name), item.content_as_bytes())
                .map_err(|err| format!("js sourcemap write error: {err}"))?;
            continue;
        }
        let final_name = file_name_output_format(&stem, "js", item.content_as_bytes(), options);
        if js_name.is_none() {
            fs::write(dist_dir.join(&final_name), item.content_as_bytes())
                .map_err(|err| format!("js output write error: {err}"))?;
            js_name = Some(final_name);
        } else {
            fs::write(dist_dir.join(name), item.content_as_bytes())
                .map_err(|err| format!("js chunk write error: {err}"))?;
        }
    }

    js_name.ok_or_else(|| format!("no js output for {}", source.display()))
}
