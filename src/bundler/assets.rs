use std::{
    fs,
    path::{Path, PathBuf},
};

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use parcel_sourcemap::SourceMap;
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, RawMinifyOptions, SourceMapType};

use super::Options;

pub async fn asset_process(
    asset_path: &Path,
    src: &Path,
    dist: &Path,
    options: Options,
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
        "css" => css_bundle(asset_path, dist, dir, options)?,
        "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" => {
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
