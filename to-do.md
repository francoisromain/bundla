# to-do

## Dev vs release bundling

Add two bundle modes to `rust/mod.rs`:

| | Dev | Release |
|---|---|---|
| minify (css/js) | no | yes |
| sourcemaps (css/js) | yes (`.map` files) | no |
| output names | stable (`styles.css`, `scripts.js`) | hashed (`styles-<hash>.css`, `scripts-<hash>.js`) |

### API
- [ ] add `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Mode { Dev, Release }`
- [ ] change signature to `bundle(src: &Path, dist: &Path, mode: Mode) -> Result<(), String>`

### Dev mode (unminified + sourcemaps)
- [ ] css: `PrinterOptions { minify: false, source_map: Some(...) }` -> `styles.css` + `styles.css.map` (append `sourceMappingURL`)
- [ ] js: `minify: Bool(false)`, `sourcemap: Some(SourceMapType::File)` -> `scripts.js` + `scripts.js.map`
- [ ] html: plain copy as today

### Release mode (minified + hashed)
- [ ] css: `minify: true`, no map, output `styles-<hash>.css`
- [ ] js: `minify: Bool(true)`, output `scripts-<hash>.js`
- [ ] html: rewrite `./styles.css` / `./scripts.js` in `src/index.html` to the hashed names (after css/js are bundled)
- [ ] hash: std-only FNV-1a over output bytes -> 8-hex-char suffix (no new dependency)
- [ ] byte-identical re-runs -> same hashes -> stable names across release builds

### Bins
- [ ] `rust/bin/dev.rs` -> call `bundle(..., Mode::Dev)`
- [ ] `rust/bin/bundle.rs` -> call `bundle(..., Mode::Release)`

### Verify
- [ ] dev: served css/js unminified, `.map` files present, webadev `event: css` hot-reload still works (stable names are required for that)
- [ ] bundle: hashed filenames in dist, html references rewritten, css/js minified, no maps
- [ ] `cargo fmt --check` + `cargo clippy --all-targets` clean

### Notes
- hashed names stay release-only: dev must keep stable filenames or webadev's css hot-swap will churn hrefs on every edit
- confirm the exact lightningcss / rolldown sourcemap output formats against the installed versions (lightningcss `to_css` map JSON + comment; rolldown `SourceMapType::File`)

## Open questions
- [ ] html minification in release? (currently skipped, would need a `minify-html` dep)
- [ ] `Mode` enum vs `Options { minify, sourcemap, hash }` struct?
- [ ] add `--mode dev|release` flag to the `bundle` bin, or keep it fixed to Release?

## Done (Option A rewrite)
- [x] real bin targets owning the bundling loop (`dev`, `bundle`)
- [x] drop dummy lib, `build.rs`, `dev.sh`, watchexec
- [x] CSS-only hot reload via webadev (source-side classification)
- [x] clean `dist/` before writing
- [x] `--port`/`--ip`/`--open` passthrough via clap