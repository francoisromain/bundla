# Bundla

<p align="center">
  <br>
  <a href="https://github.com/francoisromain/bundla">
    <img src="https://raw.githubusercontent.com/francoisromain/bundla/main/client/src/assets/bundla.svg" alt="Bundla logo" width="160">
  </a>
  <br>
</p>

Bundle HTML, CSS, and JS/TS for prod or dev with live-reload.

## Features

- Prod build `src/` into `dist/` with minification and hashed filenames,
- Dev build `src/` into `dev/` unminified with sourcemaps,
- Option to watch and rebuild on change,
- Option to filter watched files by name or suffix,
- Option to serve the bundle.

Built with Rust on the [webadev](https://crates.io/crates/webadev) server.

## Conventions

- Every `.html` file under `src` is a page entry, mirrored to `dev`/`dist`.
- `<link rel="stylesheet">`, CSS `@import` and `<script type="module">` references are bundled (one output per asset, cached across pages) and rewritten in place.
- `type="module"` is required for scripts.
- TypeScript entries work (e.g. ./scripts.ts). The rewritten ref is .js (scripts.js in dev, scripts-<hash>.js in prod).
- `?query`/`#fragment` suffixes on references are preserved.
- By default, remote and root-relative URLs are left unchanged and resolved by the browser.
- Option (`--base`) to bundle and rewrite root-relative references starting with the base prefix.
- Everything else under `src` is copied as-is (static assets), preserving the relative layout.
- The live-reload script is patched into HTTP responses, never written to `dev` or `dist`.

## CLI

```bash
cargo install bundla

# build `src/` into `dist/` (minified, hashed)
bundla
```

### Options

- `--dev` / `-d`: dev build, unminified with sourcemaps, output dir `dev/`.
- `--minify <targets>`: comma-separated output targets to minify: `css`, `js`, `html`, or `all` / `none` (default: `all` for prod builds, `none` with `--dev`). There is a [known bug](#known-bug) for templates.
- `--watch` / `-w`: build on every change to `src/`.
- `--filter <suffixes>`: comma-separated file name suffixes to watch (requires `--watch`; default: all files; e.g. `--filter html,css` or `--filter one-file.html`). An item matches when the file name equals `<item>` or ends with `.<item>`. Items are lowercased, a leading dot or path separator is rejected, and empty items are ignored.
- `--serve` / `-s`: build once, then serve the output directory.
- `--src <dir>`: source directory (default: `src`).
- `--dist <dir>`: output directory (default: `dist/`, or `dev/` with `--dev`).
- `--base <path>`: URL prefix where the output is served (default: none; must start with `/`; e.g. `/app`; normalized: trailing `/` removed, invalid segments rejected). Root-relative references starting with this base are bundled and rewritten with the prefix. With `--serve`, the built-in server serves the output under this prefix too. Use `--base` for subpath hosting (GitHub Pages project sites, reverse proxies).

#### Server config (requires `--serve`)

- `--port <n>`: port to listen on (default: random free port).
- `--ip <addr>`: IP address to bind to (default 127.0.0.1, use `0.0.0.0` to access from other devices).
- `--open`: open the page in the browser on start.
- `--index <file>`: file served by default for the root and directory requests (default: `index.html`).
- `--header`: additional HTTP header on every response (repeatable), e.g. `--header "Access-Control-Allow-Origin: *"`.

### Examples

Prod build

```bash
# prod build, watch `src/` and rebuild on change
bundla -w

# watch only html/css/js changes (static assets are copied on those rebuilds)
bundla -w --filter html,css,js

# prod build once, then serve the output
bundla -s

# prod build, watch and serve (live reload; css changes reload the full page)
bundla -ws

# prod build with custom source and output directories
bundla --src client/src --dist client/dist

# prod build with root-relative references starting with /app (e.g. `/app/styles/main.css`)
bundla --base /app

# same, served under /app
bundla -s --base /app

# prod build for a server-rendered template: minify css/js, keep the html
bundla --minify css,js
```

Dev build

```bash
# dev build of `src/` into `dev/` with sourcemaps
bundla -d

# watch: build into `dev/` on change, no server
bundla -dw

# development loop: watch `src/`, build into `dev/` on change, serve with live reload
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
    base: String::new(),
};

let server = serve(&config).await?;
server.run().await?;
```

`ServerConfig` and `Server` are re-exported from [webadev](https://crates.io/crates/webadev).

Set `config.base` to a URL prefix (e.g. `"/app"`) to serve under it. An empty string serves at the root.

Use `0` as `config.port` to let the OS pick a free port (actual port visible in `server.url`).

### 2. Build only

```rust
use std::path::Path;
use bundla::{BuildOptions, build};

let warnings = build(Path::new("src"), Path::new("dist"), &BuildOptions::PROD).await?;
for warning in warnings {
    eprintln!("warning: {warning}");
}
```

On success, `build` returns the warnings collected during the build (unreadable assets, non-module scripts, bundler warnings).

`BuildOptions` presets:

- `BuildOptions::DEV`: no minification, with sourcemaps, no hashed filenames.
- `BuildOptions::PROD`: minifies CSS, JS, and HTML, hashed filenames, no sourcemaps.

Combine a `BuildOptions` preset with a base:

```rust
let options = BuildOptions {
    base: Some("/app".into()),
    ..BuildOptions::DEV
};
```

Build your own options:

```rust
use bundla::{BuildOptions, Minify};

let options = BuildOptions {
    minify: Minify::ALL,
    sourcemap: true,
    hash: false,
    base: None,
};
```

`BuildOptions::minify` is a set of targets:

```rust
use bundla::Minify;

// minify everything (the prod default)
let all = Minify::ALL;

// minify css and js, keep the html untouched
let assets = Minify::CSS | Minify::JS;

// parse the CLI form
let targets: Minify = "css,js".parse().unwrap();
```

### 3. Watch and rebuild, no serve

```rust
use std::path::Path;
use tokio::sync::broadcast;
use bundla::{BuildOptions, WatchLog, Watch, watch};

let (tx_log, mut rx_log) = broadcast::channel(100);

// Build `src/` into `dev/` on startup, then on every change of an
// `.html`, `.css` or `.js` file; pass `&[]` to watch every file
let watcher = watch(
    Path::new("src"),
    Path::new("dev"),
    &BuildOptions::DEV,
    tx_log,
    &["html", "css", "js"],
)
.await?;

// runtime logs are reported on your broadcast channel:
// (sent on `tx_log`, consumed with `rx_log.recv()`)
// - `WatchLog::Built(ReloadType, Vec<PathBuf>)`: successful builds (reload type + changed files)
// - `WatchLog::Warn(String)`: build warnings
// - `WatchLog::Error(String)`: build failures during watch (previous build is kept)

tokio::spawn(async move {
    while let Ok(log) = rx_log.recv().await {
        eprintln!("log: {log:?}");
    }
});

watcher.run().await?; // rebuilds until the future is dropped
```

`Watch` is the handle returned by `watch`. `watch_serve` below runs the same rebuild loop behind the server, with the same name/suffix filter (`&[]` watches every file).

### 4. Watch, rebuild and serve (live reload)

```rust
use std::net::IpAddr;
use std::path::Path;
use tokio::sync::broadcast;
use bundla::{BuildOptions, WatchLog, ServerConfig, watch_serve};

let config = ServerConfig {
    dir: "dev".into(),
    ip: IpAddr::from([127, 0, 0, 1]),
    port: 8080,
    headers: vec![],
    index: "index.html".into(),
    base: String::new(),
};

let (tx_log, mut rx_log) = broadcast::channel(100);

// `BuildOptions::DEV` for the dev loop; `&[]` watches every file
let server = watch_serve(Path::new("src"), &config, &BuildOptions::DEV, tx_log, &[]).await?;
println!("Starting development server at {}", server.url);

// runtime logs are reported on your broadcast channel:
// (sent on `tx_log`, consumed with `rx_log.recv()`)
// - `WatchLog::Built(ReloadType, Vec<PathBuf>)`: successful builds (reload type + changed files)
// - `WatchLog::Warn(String)`: build warnings
// - `WatchLog::Error(String)`: build failures during watch (previous build is kept)

tokio::spawn(async move {
    while let Ok(log) = rx_log.recv().await {
        eprintln!("log: {log:?}");
    }
});

server.run().await?; // blocks until shutdown
```

The served prefix `ServerConfig.base` has to match the build prefix `BuildOptions.base`.

When building and serving manually, use `BuildOptions.server_base()` to populate `ServerConfig.base` (`String`) from `options.base` (`Option`) and ensure consistency.

In `watch_serve`, `ServerConfig.base` is ignored and overwritten by `BuildOptions.base` using `BuildOptions.server_base()`.

### Feature flag

The default features:
- are required for: `serve`, `watch_serve`, `Server` and `ServerConfig`,
- are not required for: `build`, `watch`, `Watch`, `WatchLog`, `Minify` and `BuildOptions`.

To `build` or `watch` without the server (and drop the axum dependency):

```toml
bundla = { version = "0.7", default-features = false }
```

## Local installation

```bash
# clone the repo
git clone https://github.com/francoisromain/bundla.git
cd bundla

# build the project
cargo build --release

# watch `client/src`, serve `client/dev`
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

## Known bug

### HTML minification and templates

HTML is minified by [minify-html](https://crates.io/crates/minify-html), which parses the inside of a start tag as an attribute list. Template tags placed between attributes can therefore be corrupted. This is an upstream bug in minify-html, not a bundla limitation: [wilsonzlin/minify-html#232](https://github.com/wilsonzlin/minify-html/issues/232).

If your templates place tags inside HTML start tags (e.g. conditional boolean attributes), leave the `html` target out for now:

```bash
bundla --minify css,js
```
