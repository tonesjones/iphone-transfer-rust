use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
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
                "Photos, videos, and related files found in the source folder: {}",
                report.found
            );
            if report.copied == 0 {
                println!("No files needed copying.");
            } else {
                println!("Files copied to your saved library: {}", report.copied);
            }
            println!("Files already saved and checked: {}", report.skipped);
            if report.failed > 0 {
                println!("Could not back up {} files or folders:", report.failed);
            }
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
                "Saved library: {} files checked and unchanged.",
                report.verified_library_files
            );
            println!(
                "Source folder: {} files have matching saved copies.",
                report.safe.len()
            );
            let clean = report.missing.is_empty()
                && report.library_problems.is_empty()
                && report.untracked.is_empty()
                && report.errors.is_empty();
            if clean {
                println!("No missing, changed, unrecorded, or unreadable files found.");
            } else {
                println!(
                    "Needs attention: {} source files without verified copies, {} missing or changed saved files, {} unrecorded saved files, {} read errors.",
                    report.missing.len(),
                    report.library_problems.len(),
                    report.untracked.len(),
                    report.errors.len()
                );
            }
            for path in &report.missing {
                println!("No verified saved copy: {}", display_path(path));
            }
            for problem in &report.library_problems {
                println!("Saved copy needs attention: {problem}");
            }
            for path in &report.untracked {
                println!("File missing from the catalog: {}", display_path(path));
            }
            for error in &report.errors {
                println!("Could not read: {error}");
            }
            if !clean {
                std::process::exit(1);
            }
        }
        Command::Find { text, to } => {
            let db = photoxfer_core::db::Db::open_read_only(&to.join("library.db"))?;
            let matches = db.find(&text)?;
            for (original, relative) in &matches {
                println!("{original}\t{}", display_path(&to.join(relative)));
            }
            println!("{} matching source references", matches.len());
        }
        Command::Archive { from, to } => {
            let report = photoxfer_core::archive::archive(&from, &to)?;
            println!(
                "Second copy: {} saved files checked against your library; {} needed copying.",
                report.verified, report.copied
            );
            println!("Recovery file list: {}", display_path(&report.manifest));
            println!(
                "This checks the second copy on this computer. It does not check OneDrive's cloud copy."
            );
        }
        Command::Verify { from, manifest } => {
            let count = photoxfer_core::archive::verify(&from, &manifest)?;
            println!(
                "All {count} files in the recovery file list match, including the database backup.\nThis confirms cloud recovery only if this folder was downloaded separately from OneDrive."
            );
        }
    }
    Ok(())
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    }
}
