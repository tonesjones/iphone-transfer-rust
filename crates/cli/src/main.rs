use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    prelude::*,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Import {
        #[arg(long = "from", required = true)]
        from: PathBuf,
        #[arg(long = "to", required = true)]
        to: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    // nom_exif logs every EXIF entry it parses; keep it quiet and send the rest to stderr.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            Targets::new()
                .with_default(Level::WARN)
                .with_target("nom_exif", LevelFilter::OFF),
        )
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Import { from, to } => {
            let report = photoxfer_core::import::import_folder(&from, &to)?;
            println!(
                "found {}, copied {}, skipped {}, failed {}, ignored {} non-media",
                report.found, report.copied, report.skipped, report.failed, report.ignored
            );
            for error in report.errors {
                println!("{error}");
            }
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
