# Bundla

<p align="center">
  <br>
  <a href="https://github.com/francoisromain/bundla">
    <img src="https://raw.githubusercontent.com/francoisromain/bundla/main/client/src/assets/bundla.svg" alt="Bundla logo" width="160">
  </a>
  <br>
</p>

Bundle HTML, CSS, and JS/TS:
- development mode with live-reload,
- release mode with minification and hashed filenames.

Built with rust on the [webadev](https://crates.io/crates/webadev) library.

## Development mode (default)

- Watch `src` and re-bundle on every change.
- Output to `dev` and serve.
- Hot-reload after CSS edits, full page reload for HTML and JS/TS.

## Release mode

- Bundle, minify, hash filenames from `src`.
- Output to `dist`.
- Option: serve the `dist` directory.

## Conventions

- Every `.html` file under `src` is a page entry, mirrored to `dev`/`dist`.
- `<link rel="stylesheet">` and `<script type="module">` references are bundled (one output per asset, cached across pages) and rewritten in place; `type="module"` is required for scripts.
- The source layout is mirrored in the output and `?query`/`#fragment` suffixes on references are preserved.
- Relative css `url()` and css-to-css `@import` targets are bundled and rewritten; remote and absolute URLs are left for the browser to resolve.
- Everything else under `src` is copied as-is (static assets).
- `dev`/`dist` must not overlap `src`.
- The build renders into a temporary sibling of `dev`/`dist` and swaps it in only on success, so a failed build never leaves a partial output.

## CLI

```bash
cargo install bundla

# bundle `src/`, serve `dev/`
bundla
```

### Options

- `--release`: bundle, minify, hash filenames, to output dir (default: `dist`).
- `--serve`: serve the output directory (default: `dist`) without watching or bundling.
- `--src <dir>`: assets directory to watch and bundle (default: `src`).
- `--dist <dir>`: output or serving directory (default to `dev/` or `dist/` with `--release`/`--serve`).
- `--port <n>` default 8080 (use `0` for a random free port).
- `--ip <addr>` default 127.0.0.1 (use `0.0.0.0` to access from other devices).
- `--open`: open the page in the browser on start.
- `--index <file>`: file served by default for the root and directory requests (default: `index.html`; no flag + no index file → 404 at `/`).
- `--header`: additional HTTP header on every response (repeatable), e.g. `--header "Access-Control-Allow-Origin: *"`.

### Examples

```bash
# dev server (watch `src/` → bundle → serve `dev/` with live reload
bundla
bundla --port 9000 --open

# release build of `src/` to `dist/`
bundla --release

# serve an existing `dist/` without bundling
bundla --serve

# release build, then serve it
bundla --release --serve

# bundle and serve dist/ in dev mode
bundla --dist dist   
```

## Library

### 1. Serve only

```rust
use bundla::{ServerConfig, serve};

let config = ServerConfig {
    dir: "dev".into(),
    ip: IpAddr::from([127, 0, 0, 1]),
    port: 8080,
    headers: vec![],
    index: "index.html".into(),
};

let server = serve(&config).await?;
server.run().await?;
```

`ServerConfig` and `Server` are re-exported from [webadev](https://crates.io/crates/webadev).

Use `0` as `config.port` to let the OS pick a free port (visible in `server.url`).

### 2. Bundle

```rust
use std::path::Path;

use bundla::{BundlerOptions, bundle};

let warnings = bundle(Path::new("src"), Path::new("dist"), BundlerOptions::RELEASE).await?;
for warning in warnings {
    eprintln!("warning: {warning}");
}
```

- `BundlerOptions::DEV` (default): unminified, with sourcemaps, no hashed filenames.
- `BundlerOptions::RELEASE`: minified, hashed filenames, no sourcemaps.
- `Ok` returns the warnings collected during the build (unreadable assets, non-module scripts, bundler warnings); displaying them is up to you.

Build a custom profile from the flags:

```rust
BundlerOptions {
    minify: true,
    sourcemap: true,
    hash: false,
}
```

### 3. Dev server with live reload

```rust
use std::net::IpAddr;
use std::path::Path;

use bundla::{DevLog, ServerConfig, dev};
use tokio::sync::broadcast;

let config = ServerConfig {
    dir: "dev".into(),
    ip: IpAddr::from([127, 0, 0, 1]),
    port: 8080,
    headers: vec![],
    index: "index.html".into(),
};

// runtime logs are reported on your broadcast channel:
// - `DevLog::Change(ReloadType, Vec<PathBuf>)`: source changes from the watcher
// - `DevLog::Warn(String)`: bundle warnings
// - `DevLog::Error(String)`: rebundle failures during watch (previous bundle is kept)
let (tx_log, mut rx_log) = broadcast::channel(100);
let server = dev(Path::new("src"), &config, tx_log).await?; // bundles, watches, serves
println!("Starting development server at {}", server.url);

tokio::spawn(async move {
    while let Ok(log) = rx_log.recv().await {
        eprintln!("log: {log:?}");
    }
});

server.run().await?; // blocks until shutdown
```

## Local installation

```bash
# clone the repo
git clone https://github.com/francoisromain/bundla.git
cd bundla

# build the project
cargo build --release

# in the `client` directory, bundle `src/`, serve `dev/`
./target/release/bundla --src client/src --dist client/dev --header "Access-Control-Allow-Origin: *"

# install globally from the local package
# compiles and copies the binary to `~/.cargo/bin/bundla`
cargo install --path . --locked

# use from anywhere
# from the current directory, bundle `src/`, serve `dev/` on 127.0.0.1:8080
cd ~/some/project
bundla           

# update later after source changes
cargo install --path . --locked --force
```

