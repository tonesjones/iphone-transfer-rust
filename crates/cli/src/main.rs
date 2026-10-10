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
    /// Confirm every media file in --from has a verified copy in the library. Read-only.
    Check {
        #[arg(long = "from", required = true)]
        from: PathBuf,
        #[arg(long = "to", required = true)]
        to: PathBuf,
    },
    /// Find a saved file by its original name or source path.
    Find {
        text: String,
        #[arg(long)]
        to: PathBuf,
    },
    /// Copy verified media and a database snapshot to a separate archive folder.
    Archive {
        #[arg(long = "from")]
        from: PathBuf,
        #[arg(long = "to")]
        to: PathBuf,
    },
    /// Verify every file in an archive manifest, including a cloud-restored batch.
    Verify {
        #[arg(long = "from")]
        from: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
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
        Command::Check { from, to } => {
            let report = photoxfer_core::check::check(&from, &to)?;
            println!(
                "{} source files verified in library, {} not in library, {} library problems, {} untracked library files, {} unreadable, ignored {} non-media",
                report.safe.len(),
                report.missing.len(),
                report.library_problems.len(),
                report.untracked.len(),
                report.errors.len(),
                report.ignored
            );
            for path in &report.missing {
                println!("not in library: {}", path.display());
            }
            for problem in &report.library_problems {
                println!("library problem: {problem}");
            }
            for path in &report.untracked {
                println!("untracked: {}", path.display());
            }
            for error in &report.errors {
                println!("unreadable: {error}");
            }
            if !(report.missing.is_empty()
                && report.library_problems.is_empty()
                && report.untracked.is_empty()
                && report.errors.is_empty())
            {
                std::process::exit(1);
            }
        }
        Command::Find { text, to } => {
            let db = photoxfer_core::db::Db::open_read_only(&to.join("library.db"))?;
            let matches = db.find(&text)?;
            for (original, relative) in &matches {
                println!("{original}\t{}", to.join(relative).display());
            }
            println!("{} matching source references", matches.len());
        }
        Command::Archive { from, to } => {
            let report = photoxfer_core::archive::archive(&from, &to)?;
            println!(
                "{} media files verified in local archive; {} newly copied",
                report.verified, report.copied
            );
            println!("Manifest: {}", report.manifest.display());
            println!(
                "Cloud upload and restore are not verified. Cleanup remains blocked until phone completeness and a cloud-restored batch are verified."
            );
        }
        Command::Verify { from, manifest } => {
            let count = photoxfer_core::archive::verify(&from, &manifest)?;
            println!(
                "{count} files match the archive manifest. This proves cloud recovery only if --from is a separate download from OneDrive."
            );
        }
    }
    Ok(())
}
