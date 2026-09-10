use std::path::PathBuf;
use std::process::exit;

use clap::Parser;

use webadev_bundle::bundle;

#[derive(Parser)]
#[command(about = "bundle the html/css/js assets into dist")]
struct Args {
    /// assets directory
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// output directory
    #[arg(long, default_value = "dist")]
    dist: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    match bundle(&args.src, &args.dist).await {
        Ok(()) => println!("bundled {}", args.dist.display()),
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    }
}
