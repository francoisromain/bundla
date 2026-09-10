use std::{fs, path::Path};

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, RawMinifyOptions};

/// bundle the html/css/js assets from `src` into `dist`.
/// `dist` is cleaned first, so stale files never linger.
pub async fn bundle(src: &Path, dist: &Path) -> Result<(), String> {
    clean_dist(dist)?;
    html_copy(src, dist)?;
    css_bundle(src, dist)?;
    js_bundle(src, dist).await?;
    Ok(())
}

fn clean_dist(dist: &Path) -> Result<(), String> {
    if dist.exists() {
        fs::remove_dir_all(dist)
            .map_err(|err| format!("failed to clean {}: {err}", dist.display()))?;
    }
    fs::create_dir_all(dist).map_err(|err| format!("failed to create {}: {err}", dist.display()))
}

fn html_copy(src: &Path, dist: &Path) -> Result<(), String> {
    let entry = src.join("index.html");
    if !entry.is_file() {
        return Ok(());
    }
    fs::copy(&entry, dist.join("index.html"))
        .map(|_| ())
        .map_err(|err| format!("failed to copy index.html: {err}"))
}

fn css_bundle(src: &Path, dist: &Path) -> Result<(), String> {
    let entry_file = src.join("styles/styles.css");
    if !entry_file.is_file() {
        return Ok(());
    }

    let css_code =
        fs::read_to_string(&entry_file).map_err(|e| format!("css source unreadable: {e}"))?;
    let mut stylesheet = StyleSheet::parse(&css_code, ParserOptions::default())
        .map_err(|e| format!("css parse error: {e}"))?;
    stylesheet
        .minify(MinifyOptions::default())
        .map_err(|e| format!("css minify error: {e}"))?;
    let result = stylesheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .map_err(|e| format!("css print error: {e}"))?;

    fs::write(dist.join("styles.css"), result.code)
        .map_err(|e| format!("css output write error: {e}"))
}

async fn js_bundle(src: &Path, dist: &Path) -> Result<(), String> {
    let src_dir = src.join("scripts");
    if !src_dir.is_dir() {
        return Ok(());
    }
    let entry_name = if src_dir.join("scripts.ts").is_file() {
        "scripts.ts"
    } else if src_dir.join("scripts.js").is_file() {
        "scripts.js"
    } else {
        return Ok(());
    };

    let output_file = dist
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", dist.display()))?
        .join("scripts.js");
    let cwd = src_dir
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", src_dir.display()))?;
    let mut bundler = Bundler::new(BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("scripts".to_string()),
            import: entry_name.to_string(),
        }]),
        cwd: Some(cwd),
        file: Some(output_file.to_string_lossy().into_owned()),
        format: Some(OutputFormat::Iife),
        minify: Some(RawMinifyOptions::Bool(true)),
        ..BundlerOptions::default()
    })
    .map_err(|err| format!("bundler init error: {err}"))?;

    let output = bundler
        .write()
        .await
        .map_err(|err| format!("js bundle error: {err}"))?;

    for warning in output.warnings {
        eprintln!("js bundle warning: {warning}");
    }

    Ok(())
}
