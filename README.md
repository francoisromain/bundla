# webadev bundle

A tiny setup to bundle Html, Css, and Js files and serve with live reload.

Built on the [webadev](https://crates.io/crates/webadev) library.

## Usage

- watches `src/`,
- rebundles on every change,
- serves `dev/` or `dist/`
- live reload: CSS edits hot-reload the stylesheet, anything else does a full page reload.

If a bundle fails, the browser stays on the last good one.

### Options

- `--src <dir>`: assets directory to watch and bundle (default: `src`)
- `--dist <dir>`: output or serving directory (default: `dev/` or `dist/` with `--release`/`--serve`)
- `--release`: bundle, minify, hash filenames, to output dir (default: `dist`)
- `--serve`: serve the output directory (default: `dist`) without watching or bundling
- `--port <n>` default 8080 (use `0` for a random free port)
- `--ip <addr>` default 127.0.0.1 (use `0.0.0.0` to access from other devices)
- `--open`: open the page in the browser on start
- `--header`: additional HTTP header on every response (repeatable), e.g. `--header "Access-Control-Allow-Origin: *"`

### Examples


```bash
# dev server (watch `src/` → bundle → serve `dev/` with live reload
cargo run
cargo run -- --port 9000 --open

# production build of `src/` to `dist/`, then exit
cargo run -- --release

# serve an existing `dist/` without bundling
cargo run -- --serve

# production build, then serve it
cargo run -- --release --serve

# serve dist/ in dev mode (rebundles dev output into it)
cargo run -- --dist dist   
```

## Layout

```
src/            assets to bundle (index.html, styles/, scripts/)
dev/            dev-server output (unminified + sourcemaps, gitignored)
dist/           release output (minified, hashed, gitignored)
rust/
  mod.rs        the bundling logic (bundle)
  bin/bundle.rs dev server, --release, or --serve
```