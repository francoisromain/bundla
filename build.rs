use std::{env, fs, path::Path};

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, RawMinifyOptions};
use tokio::runtime::Runtime;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let src_dir = Path::new(&manifest_dir).join("src");
    let dist_dir = Path::new(&manifest_dir).join("dist");

    println!("cargo:rerun-if-changed={}", src_dir.display());

    html_copy(&src_dir, &dist_dir);
    css_bundle(&src_dir, &dist_dir);
    js_bundle(&src_dir, &dist_dir);
}

fn html_copy(src_dir: &Path, dist_dir: &Path) {
    let entry = src_dir.join("index.html");
    if !entry.is_file() {
        return;
    }
    fs::copy(&entry, dist_dir.join("index.html")).expect("failed to copy index.html");
}

fn css_bundle(src_dir: &Path, dist_dir: &Path) {
    let entry_file = src_dir.join("styles/styles.css");
    if !entry_file.is_file() {
        return;
    }
    println!("cargo:rerun-if-changed={}", entry_file.display());

    let css_code = fs::read_to_string(&entry_file).expect("css source unreadable");
    let mut stylesheet =
        StyleSheet::parse(&css_code, ParserOptions::default()).expect("css parse error");
    stylesheet
        .minify(MinifyOptions::default())
        .expect("css minify error");
    let result = stylesheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .expect("css print error");

    let output_file = dist_dir.join("styles.css");
    fs::write(&output_file, result.code).expect("css output write error");
}

fn js_bundle(src_dir: &Path, dist_dir: &Path) {
    let src_dir = src_dir.join("scripts");
    if !src_dir.is_dir() {
        return;
    }
    let entry_name = if src_dir.join("scripts.ts").is_file() {
        "scripts.ts"
    } else if src_dir.join("scripts.js").is_file() {
        "scripts.js"
    } else {
        return;
    };
    println!("cargo:rerun-if-changed={}", src_dir.display());

    let output_file = dist_dir.join("scripts.js");
    let mut bundler = Bundler::new(BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("scripts".to_string()),
            import: entry_name.to_string(),
        }]),
        cwd: Some(src_dir.to_path_buf()),
        file: Some(output_file.to_string_lossy().into_owned()),
        format: Some(OutputFormat::Iife),
        minify: Some(RawMinifyOptions::Bool(true)),
        ..BundlerOptions::default()
    })
    .expect("bundler init error");

    let runtime = Runtime::new().expect("tokio runtime error");
    let output = runtime.block_on(bundler.write()).expect("js bundle error");

    for warning in output.warnings {
        eprintln!("js bundle warning: {warning}");
    }
}
