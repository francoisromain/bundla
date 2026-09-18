mod css;
mod js;

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::Options;

use css::css_bundle;
use js::js_bundle;

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
