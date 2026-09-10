# webadev bundle

A tiny setup to bundle `src/` into `dist/` and serve it with live reload.

Built on the [webadev](https://crates.io/crates/webadev) library.

## Usage

### Dev server (watch + bundle + serve + live reload)

The `dev` binary 
- watches `src/`, 
- rebundles on every change, 
- serves `dist/` 
- live reload: CSS edits hot-reload the stylesheet, anything else does a
full page reload.

If a bundle fails, the browser stays on the last good one.

Options

- `--src <dir>`: assets directory to watch and bundle (default: `src`)
- `--dist <dir>`: directory to serve (default: `dist`)
- `--port <n>` default 8080 (use `0` for a random free port)
- `--ip <addr>` default 127.0.0.1 (use `0.0.0.0` to access from other devices)
- `--open`: open the page in the browser on start

Example

```bash
cargo run
cargo run -- --port 9000 --open
```

### Build artifacts only

The `bundle` binary bundles `src/` → `dist/` once and exits — no watch, no server.

```bash
cargo run --bin bundle
```

## Layout

```
src/            assets to bundle (index.html, styles/, scripts/)
dist/           bundled output (cleaned on every run, gitignored)
rust/
  mod.rs        the bundling logic (bundle)
  bin/dev.rs    dev server: watch src -> bundle -> live reload
  bin/bundle.rs artifact-only command
```
