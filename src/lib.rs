//! Bundle HTML, CSS, and JS/TS:
//! - prod build with minification and hashed filenames,
//! - dev build with sourcemaps,
//! - watch-and-rebuild, headless or served with live reload.
//!
//! See the [README](https://crates.io/crates/bundla)
//! for features, CLI, and library usage.

#![warn(missing_docs)]

/// Alias of `webadev::Config`, re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::Config as ServerConfig;

/// Alias of `webadev::ReloadType`, re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::ReloadType;

/// Alias of `webadev::Server`, re-exported from [webadev](https://crates.io/crates/webadev).
pub use webadev::Server;

mod bundler;

pub use bundler::{BundlerOptions, bundle};

mod watch;

pub use watch::{Watch, WatchLog, watch};

mod server;

pub use server::{serve, watch_serve};
