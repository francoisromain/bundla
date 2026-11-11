use std::path::{Path, PathBuf};

use tokio::sync::broadcast::{self, Sender};
use webadev::ReloadType;

use crate::{BuildOptions, build};

/// A runtime log line from the watch loop, sent on the caller's broadcast
/// channel so the library never prints directly.
#[derive(Clone, Debug)]
pub enum WatchLog {
    /// A source change detected by the watcher, before the rebuild.
    Change(ReloadType, Vec<PathBuf>),
    /// A non-fatal build warning (unreadable asset skipped, etc.).
    Warn(String),
    /// A rebuild failure during watch; the browser keeps the previous build.
    Error(String),
}

/// Headless watch handle returned by [`watch`].
///
/// No server is involved: no port, no HTTP, no browser. The rebuild
/// loop runs while the [`Watch::run`] future is alive; dropping it stops
/// watching. Nothing is ever injected into the written files.
#[derive(Debug)]
pub struct Watch {
    rx_src: broadcast::Receiver<(ReloadType, Vec<PathBuf>)>,
    src: PathBuf,
    dist: PathBuf,
    options: BuildOptions,
    tx_log: Sender<WatchLog>,
    // Dist-change events for the server: a successful rebuild
    // triggers a browser reload. `None` when headless.
    tx_dist: Option<Sender<(ReloadType, Vec<PathBuf>)>>,
}

/// Watch `src` and rebuild it into `dist` on startup and on every
/// change, without serving. Headless: no server, no port, no browser.
///
/// The first build runs before `watch` returns, so a failure aborts
/// with `Err`. Afterwards every detected change triggers a rebuild; a
/// failed rebuild is reported on `tx_log` as [`WatchLog::Error`] and the
/// previous output is kept.
///
/// Runtime logs are reported on `tx_log`:
/// - [`WatchLog::Change`]: source changes from the watcher
/// - [`WatchLog::Warn`]: build warnings
/// - [`WatchLog::Error`]: rebuild failures (previous build kept)
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use bundla::{BuildOptions, WatchLog, watch};
/// use tempfile::tempdir;
/// use tokio::sync::broadcast;
///
/// #[tokio::main]
/// async fn main() {
///     let dir = tempdir().unwrap();
///     let src = dir.path().join("src");
///     std::fs::create_dir_all(&src).unwrap();
///     std::fs::write(src.join("index.html"), "<html><body>v1</body></html>").unwrap();
///
///     let (tx_log, _rx_log) = broadcast::channel::<WatchLog>(100);
///     let dist = dir.path().join("dev");
///     let watcher = watch(&src, &dist, BuildOptions::DEV, tx_log).await.unwrap();
///
///     // `src` is built once before `watch` returns
///     assert!(std::fs::read_to_string(dist.join("index.html")).unwrap().contains("v1"));
///
///     // `run` rebuilds on every change, until its future is dropped
///     let handle = tokio::spawn(watcher.run());
///     std::fs::write(src.join("index.html"), "<html><body>v2</body></html>").unwrap();
///     handle.abort(); // stop watching
/// }
/// ```
pub async fn watch(
    src: &Path,
    dist: &Path,
    options: BuildOptions,
    tx_log: Sender<WatchLog>,
) -> Result<Watch, String> {
    watch_setup(src, dist, options, tx_log, None).await
}

// Shared by the headless `watch` and the server `watch_serve`:
// - start the filesystem watcher on `src`
// - build once on startup
// - keep the change receiver for the rebuild loop (`Watch::run`)
//
// `tx_dist` forwards dist-change events to the server (browser
// reload); `None` when headless.
pub(crate) async fn watch_setup(
    src: &Path,
    dist: &Path,
    options: BuildOptions,
    tx_log: Sender<WatchLog>,
    tx_dist: Option<Sender<(ReloadType, Vec<PathBuf>)>>,
) -> Result<Watch, String> {
    // Src-change events: when the user updates files in the src directory
    let (tx_src, rx_src) = broadcast::channel(100);

    webadev::watch(tx_src, src)
        .map_err(|err| format!("failed to watch {}: {err}", src.display()))?;

    // Build once on startup
    build(src, dist, options)
        .await
        .map_err(|err| format!("build error: {err}"))?
        .into_iter()
        .for_each(|warning| {
            let _ = tx_log.send(WatchLog::Warn(warning));
        });

    Ok(Watch {
        rx_src,
        src: src.to_path_buf(),
        dist: dist.to_path_buf(),
        options,
        tx_log,
        tx_dist,
    })
}

impl Watch {
    /// rebuild on every source change, forever, until the future is
    /// dropped.
    ///
    /// Each detected change is reported on the log channel as
    /// [`WatchLog::Change`], then the whole `src` directory is rebuilt.
    /// Warnings are reported as [`WatchLog::Warn`]; a failed rebuild is
    /// reported as [`WatchLog::Error`] and the previous output is kept.
    pub async fn run(mut self) -> Result<(), String> {
        while let Ok((reload_type, paths)) = self.rx_src.recv().await {
            let _ = self
                .tx_log
                .send(WatchLog::Change(reload_type, paths.clone()));

            match build(&self.src, &self.dist, self.options).await {
                Ok(warnings) => {
                    warnings.into_iter().for_each(|warning| {
                        let _ = self.tx_log.send(WatchLog::Warn(warning));
                    });

                    // Successful rebuild: reload the browser when serving
                    if let Some(tx_dist) = &self.tx_dist {
                        let _ = tx_dist.send((browser_reload(reload_type, self.options), paths));
                    }
                }
                Err(err) => {
                    // Keep the previous build
                    let _ = self
                        .tx_log
                        .send(WatchLog::Error(format!("build error: {err}")));
                }
            }
        }

        Ok(())
    }
}

// How a successful rebuild tells connected browsers to reload.
//
// Css hot-reload re-fetches the stylesheet href held by the page, so it
// only works with stable filenames: with hashed filenames the rebuild
// serves a new name (and the rebuild replaces the whole output), so the
// stale href would 404 — ask for a full page reload instead, which
// picks up the fresh hashed references.
fn browser_reload(reload_type: ReloadType, options: BuildOptions) -> ReloadType {
    if options.hash {
        ReloadType::Page
    } else {
        reload_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_css_changes_reload_full_page() {
        assert_eq!(
            browser_reload(ReloadType::Css, BuildOptions::PROD),
            ReloadType::Page
        );
        assert_eq!(
            browser_reload(ReloadType::Page, BuildOptions::PROD),
            ReloadType::Page
        );
    }

    #[test]
    fn unhashed_css_changes_hot_reload() {
        assert_eq!(
            browser_reload(ReloadType::Css, BuildOptions::DEV),
            ReloadType::Css
        );
        assert_eq!(
            browser_reload(ReloadType::Page, BuildOptions::DEV),
            ReloadType::Page
        );
    }
}
