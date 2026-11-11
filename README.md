# Bundla

<p align="center">
  <br>
  <a href="https://github.com/francoisromain/bundla">
    <img src="https://raw.githubusercontent.com/francoisromain/bundla/main/client/src/assets/bundla.svg" alt="Bundla logo" width="160">
  </a>
  <br>
</p>

Bundle HTML, CSS, and JS/TS:
- prod build with minification and hashed filenames,
- dev build with sourcemaps,
- watch option to rebuild on change,
- serve option.

Built with Rust on the [webadev](https://crates.io/crates/webadev) library.

## Production build (default)

Builds `src/` into `dist/`. Files are minified, with hashed filenames.

## Development build

Builds `src/` into `dev/`. Files are unminified with sourcemaps.

## Conventions

- Every `.html` file under `src` is a page entry, mirrored to `dev`/`dist`.
- `<link rel="stylesheet">` and `<script type="module">` references are bundled (one output per asset, cached across pages) and rewritten in place; `type="module"` is required for scripts.
- The source layout is mirrored in the output and `?query`/`#fragment` suffixes on references are preserved.
- Relative css `url()` and css-to-css `@import` targets are bundled and rewritten; remote and absolute URLs are left for the browser to resolve.
- Everything else under `src` is copied as-is (static assets).
- The live-reload script is serve-time only (patched into HTTP responses, never written to `dev` or `dist`).

## CLI

```bash
cargo install bundla

# build `src/` into `dist/` (minified, hashed)
bundla
```

### Options

- `--dev` / `-d`: dev build, unminified with sourcemaps, output dir `dev/`.
- `--watch` / `-w`: rebuild on every change to `src/`.
- `--serve` / `-s`: build once, then serve the output directory.
- `--src <dir>`: directory to bundle (default: `src`).
- `--dist <dir>`: output directory (default: `dist/`, or `dev/` with `--dev`).

#### Server options (require `--serve`)

- `--port <n>`: port to listen on (default: random free port).
- `--ip <addr>`: IP address to bind to (default 127.0.0.1, use `0.0.0.0` to access from other devices).
- `--open`: open the page in the browser on start.
- `--index <file>`: file served by default for the root and directory requests (default: `index.html`).
- `--header`: additional HTTP header on every response (repeatable), e.g. `--header "Access-Control-Allow-Origin: *"`.

### Examples

Prod build

```bash
# prod build, watch `src` and rebuild on change
bundla -w

# prod build once, then serve the output
bundla -s

# prod build, watch and serve (live reload; css changes reload the full page)
bundla -ws

# custom source and output directories
bundla --src client/src --dist client/dist
```

Dev build

```bash
# dev build of `src/` into `dev/` with sourcemaps
bundla -d

# watch: rebuild `dev/` on change, no server
bundla -dw

# development loop: watch `src/`, rebuild into `dev/`, serve with live reload
bundla -dws
bundla -dws --port 9000 --open
```

## Library

### 1. Serve only

```rust
use std::net::IpAddr;
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

### 2. Prod build

```rust
use std::path::Path;
use bundla::{BundlerOptions, bundle};

let warnings = bundle(Path::new("src"), Path::new("dist"), BundlerOptions::PROD).await?;
for warning in warnings {
    eprintln!("warning: {warning}");
}
```

- `BundlerOptions::DEV`: unminified, with sourcemaps, no hashed filenames.
- `BundlerOptions::PROD`: minified, hashed filenames, no sourcemaps.
- `Ok` returns the warnings collected during the build (unreadable assets, non-module scripts, bundler warnings); displaying them is up to you.

Build a custom profile from the flags:

```rust
BundlerOptions {
    minify: true,
    sourcemap: true,
    hash: false,
}
```

### 3. Watch and rebuild, headless

```rust
use std::path::Path;
use tokio::sync::broadcast;
use bundla::{BundlerOptions, WatchLog, Watch, watch};

let (tx_log, mut rx_log) = broadcast::channel(100);

// bundles `src/` into `dev/` on startup, then on every change
// no server, no port, no browser
let watcher = watch(Path::new("src"), Path::new("dev"), BundlerOptions::DEV, tx_log).await?;

// runtime logs are reported on your broadcast channel:
// - `WatchLog::Change(ReloadType, Vec<PathBuf>)`: source changes from the watcher
// - `WatchLog::Warn(String)`: bundle warnings
// - `WatchLog::Error(String)`: rebuild failures during watch (previous build is kept)

tokio::spawn(async move {
    while let Ok(log) = rx_log.recv().await {
        eprintln!("log: {log:?}");
    }
});

watcher.run().await?; // rebuilds until the future is dropped
```

`Watch` is the handle returned by `watch`; `watch_serve` below runs the same rebuild loop behind the server.

### 4. Dev build with live reload server

```rust
use std::net::IpAddr;
use std::path::Path;
use tokio::sync::broadcast;
use bundla::{BundlerOptions, WatchLog, ServerConfig, watch_serve};

let config = ServerConfig {
    dir: "dev".into(),
    ip: IpAddr::from([127, 0, 0, 1]),
    port: 8080,
    headers: vec![],
    index: "index.html".into(),
};

// runtime logs are reported on your broadcast channel:
// - `WatchLog::Change(ReloadType, Vec<PathBuf>)`: source changes from the watcher
// - `WatchLog::Warn(String)`: bundle warnings
// - `WatchLog::Error(String)`: rebuild failures during watch (previous build is kept)
let (tx_log, mut rx_log) = broadcast::channel(100);

// `BundlerOptions::DEV` for the dev loop
let server = watch_serve(Path::new("src"), &config, BundlerOptions::DEV, tx_log).await?;
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

# in the `client` directory, watch `client/src`, serve `client/dev`
./target/release/bundla -dws --src client/src --dist client/dev --header "Access-Control-Allow-Origin: *"

# install globally from the local package
# compiles and copies the binary to `~/.cargo/bin/bundla`
cargo install --path . --locked

# use from anywhere
# from the current directory, build `src/` into `dist/`
cd ~/some/project
bundla

# update later after source changes
cargo install --path . --locked --force
```
