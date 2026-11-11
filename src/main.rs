use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::Parser;
use tokio::sync::broadcast::{self, Sender};

use bundla::{BuildOptions, Server, ServerConfig, WatchLog, build, serve, watch, watch_serve};

#[derive(Parser)]
#[command(
    about = "bundle src into dist; -d dev build, -w watch, -s serve — all combinable",
    version
)]
struct Args {
    /// Directory to bundle.
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// Output directory (default: dist/, or dev/ with --dev).
    #[arg(long)]
    dist: Option<PathBuf>,

    /// Dev build: unminified with sourcemaps (default output: dev/).
    #[arg(short, long)]
    dev: bool,

    /// Watch `src` and rebuild on every change.
    #[arg(short, long)]
    watch: bool,

    /// Serve the output directory, built once first (with --watch: live reload).
    #[arg(short, long)]
    serve: bool,

    /// Port to listen on (default: random free port).
    #[arg(
        short,
        long,
        default_value = "0",
        hide_default_value = true,
        requires = "serve"
    )]
    port: u16,

    /// IP address to bind to.
    #[arg(short, long, default_value = "127.0.0.1", requires = "serve")]
    ip: IpAddr,

    /// Open the page in the browser on start.
    #[arg(short, long, requires = "serve")]
    open: bool,

    /// File served by default for the root and directory requests.
    #[arg(long, default_value = "index.html", requires = "serve")]
    index: String,

    /// Additional HTTP header on every response (repeatable).
    #[arg(long = "header", value_name = "NAME: VALUE", requires = "serve")]
    headers: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(err) = run(&args).await {
        eprintln!("{err}");
        exit(1);
    }
}

// `--dev` selects the build profile;
// `--watch` and `--serve` select the continuations after the build.
async fn run(args: &Args) -> Result<(), String> {
    let dist = output_dir(args);
    let options = if args.dev {
        BuildOptions::DEV
    } else {
        BuildOptions::PROD
    };

    match (args.watch, args.serve) {
        // Build once, then exit
        (false, false) => build_once(&args.src, &dist, options).await,
        // Rebuild on every change, without serving
        (true, false) => {
            let tx_log = watch_log_channel();
            let watcher = watch(&args.src, &dist, options, tx_log).await?;
            println!(
                "Watching {} — rebuilding into {}",
                args.src.display(),
                dist.display()
            );
            watcher.run().await
        }
        // Build once, then serve the output
        (false, true) => {
            build_once(&args.src, &dist, options).await?;
            let server = serve(&server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
        // Rebuild on every change, served with live reload
        (true, true) => {
            let tx_log = watch_log_channel();
            let server = watch_serve(
                &args.src,
                &server_config_build(args, dist.clone()),
                options,
                tx_log,
            )
            .await?;
            println!("bundled {}", dist.display());
            start(server, args.open).await
        }
    }
}

// Build once and report the result, shared by the build-first arms.
async fn build_once(src: &Path, dist: &Path, options: BuildOptions) -> Result<(), String> {
    let warnings = build(src, dist, options).await?;
    warnings_print(&warnings);
    println!("bundled {}", dist.display());
    Ok(())
}

// Log channel with a printer task attached, shared by the watch arms.
fn watch_log_channel() -> Sender<WatchLog> {
    let (tx_log, mut rx_log) = broadcast::channel::<WatchLog>(100);
    tokio::spawn(async move {
        while let Ok(log) = rx_log.recv().await {
            watch_log_print(&log);
        }
    });
    tx_log
}

fn server_config_build(args: &Args, dist: PathBuf) -> ServerConfig {
    ServerConfig {
        dir: dist,
        ip: args.ip,
        port: args.port,
        headers: args.headers.clone(),
        index: args.index.clone(),
    }
}

// The output dir follows the build profile:
// - dist/ for the prod build (default),
// - dev/ with --dev.
// --dist overrides both.
fn output_dir(args: &Args) -> PathBuf {
    if let Some(dist) = &args.dist {
        return dist.clone();
    }

    if args.dev {
        PathBuf::from("dev")
    } else {
        PathBuf::from("dist")
    }
}

async fn start(server: Server, open: bool) -> Result<(), String> {
    println!("Serving {}", server.url);

    if open && let Err(err) = open::that(&server.url) {
        eprintln!("Failed to open browser: {err}");
    }

    server.run().await
}

fn warnings_print(warnings: &[String]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}

fn watch_log_print(log: &WatchLog) {
    match log {
        WatchLog::Change(reload_type, paths) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let message = format!("rebuilding {}", reload_type.as_str());
            for path in paths {
                let rel = path.strip_prefix(&cwd).unwrap_or(path);
                println!("Change detected: {} — {message}", rel.display());
            }
        }
        WatchLog::Warn(warning) => eprintln!("warning: {warning}"),
        WatchLog::Error(err) => eprintln!("error: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn args(
        dev: bool,
        watch: bool,
        serve: bool,
        dist: Option<PathBuf>,
        headers: Vec<String>,
    ) -> Args {
        Args {
            src: PathBuf::from("src"),
            dist,
            dev,
            watch,
            serve,
            port: 8080,
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            open: false,
            index: "index.html".to_string(),
            headers,
        }
    }

    #[test]
    fn bare_parses_as_prod_build() {
        let parsed = Args::try_parse_from(["bundla"]).unwrap();
        assert!(!parsed.dev && !parsed.watch && !parsed.serve);
    }

    #[test]
    fn flags_parse_shorts() {
        let parsed = Args::try_parse_from(["bundla", "-dws"]).unwrap();
        assert!(parsed.dev && parsed.watch && parsed.serve);

        let parsed = Args::try_parse_from(["bundla", "-dw"]).unwrap();
        assert!(parsed.dev && parsed.watch && !parsed.serve);

        let parsed = Args::try_parse_from(["bundla", "-w"]).unwrap();
        assert!(!parsed.dev && parsed.watch && !parsed.serve);

        let parsed = Args::try_parse_from(["bundla", "-s"]).unwrap();
        assert!(!parsed.dev && !parsed.watch && parsed.serve);
    }

    #[test]
    fn flags_parse_longs() {
        let parsed = Args::try_parse_from(["bundla", "--dev", "--watch", "--serve"]).unwrap();
        assert!(parsed.dev && parsed.watch && parsed.serve);
    }

    // The old --watch × --serve conflict is gone: the combo is the
    // watch-and-serve mode
    #[test]
    fn watch_and_serve_combine() {
        let parsed = Args::try_parse_from(["bundla", "--watch", "--serve"]).unwrap();
        assert!(parsed.watch && parsed.serve);
    }

    // `requires` only fires on explicit presence: the --port/--ip/--index
    // defaults must not force --serve on everyone
    #[test]
    fn port_defaults_to_random() {
        let parsed = Args::try_parse_from(["bundla"]).unwrap();
        assert_eq!(parsed.port, 0);
        assert_eq!(parsed.index, "index.html");
    }

    #[test]
    fn port_accepts_explicit_value() {
        let parsed = Args::try_parse_from(["bundla", "-s", "--port", "9000"]).unwrap();
        assert_eq!(parsed.port, 9000);
    }

    #[test]
    fn server_flags_require_serve() {
        for flag in [
            vec!["--open"],
            vec!["--header", "X: Y"],
            vec!["--port", "9000"],
            vec!["--ip", "0.0.0.0"],
            vec!["--index", "home.html"],
        ] {
            let mut argv = vec!["bundla"];
            argv.extend(&flag);

            let err = Args::try_parse_from(argv)
                .err()
                .expect("expected a server flag without --serve to be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::MissingRequiredArgument,
                "flag: {flag:?}"
            );
        }
    }

    #[test]
    fn output_dir_explicit_overrides() {
        let prod = args(false, false, false, Some(PathBuf::from("custom")), vec![]);
        assert_eq!(output_dir(&prod), PathBuf::from("custom"));

        let dev = args(true, true, true, Some(PathBuf::from("custom")), vec![]);
        assert_eq!(output_dir(&dev), PathBuf::from("custom"));
    }

    #[test]
    fn output_dir_follows_profile() {
        let prod = args(false, false, false, None, vec![]);
        assert_eq!(output_dir(&prod), PathBuf::from("dist"));

        let dev = args(true, false, false, None, vec![]);
        assert_eq!(output_dir(&dev), PathBuf::from("dev"));
    }

    #[test]
    fn output_dir_ignores_watch_and_serve() {
        let watch = args(false, true, false, None, vec![]);
        assert_eq!(output_dir(&watch), PathBuf::from("dist"));

        let watch_serve = args(false, true, true, None, vec![]);
        assert_eq!(output_dir(&watch_serve), PathBuf::from("dist"));

        let dev_watch_serve = args(true, true, true, None, vec![]);
        assert_eq!(output_dir(&dev_watch_serve), PathBuf::from("dev"));
    }

    #[test]
    fn server_config_build_maps_fields() {
        let parsed = args(
            false,
            false,
            true,
            Some(PathBuf::from("out")),
            vec!["X: Y".into()],
        );
        let config = server_config_build(&parsed, PathBuf::from("final"));

        assert_eq!(config.dir, PathBuf::from("final"));
        assert_eq!(config.ip, parsed.ip);
        assert_eq!(config.port, 8080);
        assert_eq!(config.headers, vec!["X: Y"]);
        assert_eq!(config.index, "index.html");
    }
}
