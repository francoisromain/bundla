//! Bundle Html, Css, and Js with live-reload for development, or with
//! minification and hashed filenames for release.
//!
//! See the [README](https://crates.io/crates/bundla) for features, CLI, and
//! library usage.

#![warn(missing_docs)]

/// Web server settings (`dir`, `ip`, `port`, `headers`, `index`).
///
/// Alias of `webadev::Config`, re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::Config as ServerConfig;

/// How to reload a browser tab on file change.
///
/// Re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::ReloadType;

/// A bound dev server, ready to run (see [`Server::run`]).
///
/// Re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::Server;

mod bundler;

/// Bundle options for the html/css/js output.
pub use bundler::Options as BundlerOptions;

/// Bundle the html, css, and js assets from `src` into `dist`.
pub use bundler::bundle;

mod server;

/// A runtime log line from the dev server.
pub use server::DevLog;

/// Bundle, watch, and serve `src` with live reload.
pub use server::dev;

/// Serve a directory without watching or bundling.
pub use server::serve;
