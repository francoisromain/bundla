use std::{fs, path::Path};

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use parcel_sourcemap::SourceMap;
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, RawMinifyOptions, SourceMapType};

/// Bundle options controlling css/js/html output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    /// Minify css and js output.
    pub minify: bool,
    /// Emit `.map` sourcemap files next to the output.
    pub sourcemap: bool,
    /// Hash output filenames (`styles-<hash>.css`, `scripts-<hash>.js`).
    pub hash: bool,
}

impl Options {
    /// Unminified, sourcemapped, stable filenames.
    pub const DEV: Options = Options {
        minify: false,
        sourcemap: true,
        hash: false,
    };

    /// Minified, hashed filenames, no sourcemaps.
    pub const RELEASE: Options = Options {
        minify: true,
        sourcemap: false,
        hash: true,
    };
}

/// bundle the html/css/js assets from `src` into `dist`.
/// `dist` is cleaned first, so stale files never linger.
pub async fn bundle(src: &Path, dist: &Path, options: Options) -> Result<(), String> {
    clean_dist(dist)?;
    let css_name = css_bundle(src, dist, options)?;
    let js_name = js_bundle(src, dist, options).await?;
    html_build(src, dist, css_name.as_deref(), js_name.as_deref(), options)?;
    Ok(())
}

fn clean_dist(dist: &Path) -> Result<(), String> {
    if dist.exists() {
        fs::remove_dir_all(dist)
            .map_err(|err| format!("failed to clean {}: {err}", dist.display()))?;
    }
    fs::create_dir_all(dist).map_err(|err| format!("failed to create {}: {err}", dist.display()))
}

fn css_bundle(src: &Path, dist: &Path, options: Options) -> Result<Option<String>, String> {
    let entry_file = src.join("styles/styles.css");
    if !entry_file.is_file() {
        return Ok(None);
    }

    let css_code =
        fs::read_to_string(&entry_file).map_err(|e| format!("css source unreadable: {e}"))?;
    let mut stylesheet = StyleSheet::parse(&css_code, ParserOptions::default())
        .map_err(|e| format!("css parse error: {e}"))?;
    if options.minify {
        stylesheet
            .minify(MinifyOptions::default())
            .map_err(|e| format!("css minify error: {e}"))?;
    }

    let mut map = SourceMap::new("/");
    let result = stylesheet
        .to_css(PrinterOptions {
            minify: options.minify,
            source_map: options.sourcemap.then_some(&mut map),
            ..PrinterOptions::default()
        })
        .map_err(|e| format!("css print error: {e}"))?;

    let mut code = result.code;
    if options.sourcemap {
        let map_json = map
            .to_json(None)
            .map_err(|e| format!("css sourcemap serialization error: {e}"))?;
        fs::write(dist.join("styles.css.map"), map_json)
            .map_err(|e| format!("css sourcemap write error: {e}"))?;
        code.push_str("\n/*# sourceMappingURL=styles.css.map */");
    }

    let name = if options.hash {
        format!("styles-{}.css", fnv1a(code.as_bytes()))
    } else {
        "styles.css".to_string()
    };
    fs::write(dist.join(&name), code).map_err(|e| format!("css output write error: {e}"))?;
    Ok(Some(name))
}

async fn js_bundle(src: &Path, dist: &Path, options: Options) -> Result<Option<String>, String> {
    let src_dir = src.join("scripts");
    if !src_dir.is_dir() {
        return Ok(None);
    }
    let entry_name = if src_dir.join("scripts.ts").is_file() {
        "scripts.ts"
    } else if src_dir.join("scripts.js").is_file() {
        "scripts.js"
    } else {
        return Ok(None);
    };

    let cwd = src_dir
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", src_dir.display()))?;
    let mut bundler = Bundler::new(BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("scripts".to_string()),
            import: entry_name.to_string(),
        }]),
        cwd: Some(cwd),
        format: Some(OutputFormat::Iife),
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

    let mut js_name = None;
    for item in &output.assets {
        let name = item.filename();
        if name.ends_with(".map") {
            let map_name = Path::new(name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(name);
            fs::write(dist.join(map_name), item.content_as_bytes())
                .map_err(|e| format!("js sourcemap write error: {e}"))?;
            continue;
        }
        let final_name = if options.hash {
            format!("scripts-{}.js", fnv1a(item.content_as_bytes()))
        } else {
            "scripts.js".to_string()
        };
        fs::write(dist.join(&final_name), item.content_as_bytes())
            .map_err(|e| format!("js output write error: {e}"))?;
        js_name = Some(final_name);
    }
    Ok(js_name)
}

fn html_build(
    src: &Path,
    dist: &Path,
    css_name: Option<&str>,
    js_name: Option<&str>,
    options: Options,
) -> Result<(), String> {
    let entry = src.join("index.html");
    if !entry.is_file() {
        return Ok(());
    }
    if !options.hash {
        return fs::copy(&entry, dist.join("index.html"))
            .map(|_| ())
            .map_err(|err| format!("failed to copy index.html: {err}"));
    }
    let html = fs::read_to_string(&entry).map_err(|e| format!("index.html unreadable: {e}"))?;
    let mut html = html;
    if let Some(css_name) = css_name {
        html = html.replace("styles.css", css_name);
    }
    if let Some(js_name) = js_name {
        html = html.replace("scripts.js", js_name);
    }
    fs::write(dist.join("index.html"), html).map_err(|e| format!("index.html write error: {e}"))
}

/// std-only FNV-1a 32-bit hash, formatted as 8 hex chars.
fn fnv1a(bytes: &[u8]) -> String {
    let mut hash: u32 = 0x811c9dc5;
    for &byte in bytes {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}
