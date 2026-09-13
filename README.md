# bundla

> Bundle Html, Css, and Js with live-reload for developpement, or with minification and hashed filenames for release.

> Built with rust on the [webadev](https://crates.io/crates/webadev) library.

## Features

Bundle a static website Html, Css, and Js for developpement or release.

### Developpement (default)

- watch `src`,
- re-bundle on every change,
- serve `dev`,
- hot-reload after CSS edits, full page reload for anything else.

If a bundle fails, the browser stays on the previous one.

### Release

- bundle, minify, hash filenames from `src`
- output to `dist`
- option: serve the `dist` directory 


## CLI

```bash
cargo install bundla

# bundle `src/`, serve `dev/`
bundla
```

### Options

- `--release`: bundle, minify, hash filenames, to output dir (default: `dist`)
- `--serve`: serve the output directory (default: `dist`) without watching or bundling
- `--src <dir>`: assets directory to watch and bundle (default: `src`)
- `--dist <dir>`: output or serving directory (default to `dev/` or `dist/` with `--release`/`--serve`)
- `--port <n>` default 8080 (use `0` for a random free port)
- `--ip <addr>` default 127.0.0.1 (use `0.0.0.0` to access from other devices)
- `--open`: open the page in the browser on start
- `--header`: additional HTTP header on every response (repeatable), e.g. `--header "Access-Control-Allow-Origin: *"`

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

to-do

## Local installation

```bash
# clone the repo
git clone https://github.com/francoisromain/bundla.git
cd bundla

# build the project
cargo build --release

# in the `client` directory, bundle `src/`, serve `dev/`
cd client && ./target/release/bundla --header "Access-Control-Allow-Origin: *"

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

