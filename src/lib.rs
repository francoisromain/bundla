//! Bundle HTML, CSS, and JS/TS for prod or dev with live-reload.
//! - Prod build `src/` into `dist/` with minification and hashed filenames,
//! - Dev build `src/` into `dev/` with sourcemaps,
//! - Option to watch and build on change,
//! - Option to filter watched files by name or suffix,
//! - Option to serve the bundle.
//!
//! See the [README](https://crates.io/crates/bundla)
//! for features, CLI, and library usage.

#![warn(missing_docs)]

/// Alias of `webadev::Config`, re-exported from [webadev](https://crates.io/crates/webadev).
#[cfg(feature = "serve")]
pub use webadev::Config as ServerConfig;
/// Alias of `webadev::ReloadType`, re-exported from [webadev](https://crates.io/crates/webadev).
#[cfg(any(feature = "watch", feature = "serve"))]
pub use webadev::ReloadType;
/// Alias of `webadev::Server`, re-exported from [webadev](https://crates.io/crates/webadev).
#[cfg(feature = "serve")]
pub use webadev::Server;

#[cfg(feature = "serve")]
mod serve;
#[cfg(feature = "serve")]
pub use serve::serve;

#[cfg(feature = "watch")]
mod watch;
#[cfg(feature = "watch")]
pub use watch::{BuildCause, Watch, WatchLog, watch};

#[cfg(all(feature = "serve", feature = "watch"))]
mod watch_serve;
#[cfg(all(feature = "serve", feature = "watch"))]
pub use watch_serve::watch_serve;

#[cfg(feature = "build")]
mod build;
#[cfg(feature = "build")]
pub use build::{BuildOptions, Minify, build};
