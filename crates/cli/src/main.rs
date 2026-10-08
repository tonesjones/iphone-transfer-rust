use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Import { from, to } => {
            let report = photoxfer_core::import::import_folder(&from, &to)?;
            println!(
                "found {}, copied {}, skipped {}, failed {}",
                report.found, report.copied, report.skipped, report.failed
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
