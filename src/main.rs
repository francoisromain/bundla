use std::net::IpAddr;
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;

use bundla::{BundlerOptions, Server, ServerConfig, bundle, dev, serve};

#[derive(Parser)]
#[command(
    about = "bundle src into dist; dev server by default, --release builds artifacts only",
    version
)]
struct Args {
    /// assets directory to watch and bundle
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// output or serving directory (default: dev/, or dist/ with --release or --serve)
    #[arg(long)]
    dist: Option<PathBuf>,

    /// release build: minify, hash filenames, then exit (or serve with --serve)
    #[arg(short, long)]
    release: bool,

    /// serve an output directory without rebundling (after --release: serve the fresh build)
    #[arg(long)]
    serve: bool,

    /// port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// ip address to bind to
    #[arg(short, long, default_value = "127.0.0.1")]
    ip: IpAddr,

    /// open the page in the browser on start
    #[arg(short, long)]
    open: bool,

    /// file served by default for the root and directory requests
    #[arg(long, default_value = "index.html")]
    index: String,

    /// additional HTTP header on every response (repeatable)
    #[arg(long = "header", value_name = "NAME: VALUE")]
    headers: Vec<String>,
}

enum Mode {
    Dev,
    Release,
    Serve,
    ReleaseThenServe,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(err) = run(&args).await {
        eprintln!("{err}");
        exit(1);
    }
}

async fn run(args: &Args) -> Result<(), String> {
    let dist = output_dir(args);

    if args.open && args.release && !args.serve {
        return Err("error: --open requires a running server (--serve)".to_string());
    }
    if args.release && !args.serve && !args.headers.is_empty() {
        return Err("error: --header requires a running server (--serve)".to_string());
    }

    match mode_select(args.release, args.serve) {
        Mode::Dev => {
            let server = dev(&args.src, &server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
        Mode::Release => {
            bundle(&args.src, &dist, BundlerOptions::RELEASE).await?;
            println!("bundled {}", dist.display());
            Ok(())
        }
        Mode::Serve => {
            let server = serve(&server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
        Mode::ReleaseThenServe => {
            bundle(&args.src, &dist, BundlerOptions::RELEASE).await?;
            println!("bundled {}", dist.display());
            let server = serve(&server_config_build(args, dist)).await?;
            start(server, args.open).await
        }
    }
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

fn mode_select(release: bool, serve: bool) -> Mode {
    match (release, serve) {
        (false, false) => Mode::Dev,
        (true, false) => Mode::Release,
        (false, true) => Mode::Serve,
        (true, true) => Mode::ReleaseThenServe,
    }
}

fn output_dir(args: &Args) -> PathBuf {
    if let Some(dist) = &args.dist {
        return dist.clone();
    }

    if args.release || args.serve {
        PathBuf::from("dist")
    } else {
        PathBuf::from("dev")
    }
}

async fn start(server: Server, open: bool) -> Result<(), String> {
    println!("Serving {}", server.url);

    if open && let Err(err) = open::that(&server.url) {
        eprintln!("Failed to open browser: {err}");
    }

    server.run().await
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn args(
        release: bool,
        serve: bool,
        open: bool,
        dist: Option<PathBuf>,
        headers: Vec<String>,
    ) -> Args {
        Args {
            src: PathBuf::from("src"),
            dist,
            release,
            serve,
            port: 8080,
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            open,
            index: "index.html".to_string(),
            headers,
        }
    }

    #[test]
    fn mode_select_maps_combos() {
        assert!(matches!(mode_select(false, false), Mode::Dev));
        assert!(matches!(mode_select(true, false), Mode::Release));
        assert!(matches!(mode_select(false, true), Mode::Serve));
        assert!(matches!(mode_select(true, true), Mode::ReleaseThenServe));
    }

    #[test]
    fn output_dir_explicit_overrides() {
        let explicit = args(false, false, false, Some(PathBuf::from("custom")), vec![]);
        assert_eq!(output_dir(&explicit), PathBuf::from("custom"));

        let release = args(true, false, false, Some(PathBuf::from("custom")), vec![]);
        assert_eq!(output_dir(&release), PathBuf::from("custom"));
    }

    #[test]
    fn output_dir_defaults_dev() {
        let dev = args(false, false, false, None, vec![]);
        assert_eq!(output_dir(&dev), PathBuf::from("dev"));
    }

    #[test]
    fn output_dir_defaults_release_or_serve() {
        let release = args(true, false, false, None, vec![]);
        assert_eq!(output_dir(&release), PathBuf::from("dist"));

        let serve = args(false, true, false, None, vec![]);
        assert_eq!(output_dir(&serve), PathBuf::from("dist"));

        let both = args(true, true, false, None, vec![]);
        assert_eq!(output_dir(&both), PathBuf::from("dist"));
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

    #[tokio::test]
    async fn run_rejects_open_without_server() {
        let parsed = args(true, false, true, None, vec![]);
        let err = run(&parsed).await.unwrap_err();
        assert!(
            err.contains("--open requires a running server"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn run_rejects_headers_without_server() {
        let parsed = args(true, false, false, None, vec!["X: Y".into()]);
        let err = run(&parsed).await.unwrap_err();
        assert!(
            err.contains("--header requires a running server"),
            "got: {err}"
        );
    }
}
