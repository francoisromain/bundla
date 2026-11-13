//! Bundle HTML, CSS, and JS/TS for prod or dev with live-reload.
//! - Prod build `src/` into `dist/` with minification and hashed filenames,
//! - Dev build `src/` into `dev/` with sourcemaps,
//! - Option to watch and rebuild on change,
//! - Option to serve the bundle.
//!
//! Serving is optional: the `serve` feature is on by default, and
//! `default-features = false` builds without the axum dependency.
//!
//! See the [README](https://crates.io/crates/bundla)
//! for features, CLI, and library usage.

#![warn(missing_docs)]

/// Alias of `webadev::Config`, re-exported from [webadev](https://crates.io/crates/webadev).
#[cfg(feature = "serve")]
pub use webadev::Config as ServerConfig;

/// Alias of `webadev::ReloadType`, re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::ReloadType;

/// Alias of `webadev::Server`, re-exported from [webadev](https://crates.io/crates/webadev).
#[cfg(feature = "serve")]
pub use webadev::Server;

mod build;

pub use build::{BuildOptions, build};

mod watch;

pub use watch::{Watch, WatchLog, watch};

#[cfg(feature = "serve")]
mod serve;

#[cfg(feature = "serve")]
pub use serve::{serve, watch_serve};
