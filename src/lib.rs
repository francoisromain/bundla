//! Bundle HTML, CSS, and JS/TS:
//! - development mode with live-reload,
//! - release mode with minification and hashed filenames.
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

pub use bundler::Options as BundlerOptions;

pub use bundler::bundle;

mod server;

pub use server::DevLog;

pub use server::dev;

pub use server::serve;
