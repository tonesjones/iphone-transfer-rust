use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::{
    db::Db,
    hash,
    import::{ImportReport, walk},
};

#[derive(Debug, Default)]
pub struct CheckReport {
    pub verified_library_files: usize,
    /// Source files whose exact bytes are in the library and verified on disk.
    pub safe: Vec<PathBuf>,
    /// Source files the library doesn't have (or whose library copy failed verification).
    pub missing: Vec<PathBuf>,
    /// Library files that are gone or no longer match their recorded hash.
    pub library_problems: Vec<String>,
    /// Library files the database doesn't know about.
    pub untracked: Vec<PathBuf>,
    pub ignored: u64,
    pub errors: Vec<String>,
}

impl CheckReport {
    /// True when nothing is missing, changed, unrecorded, or unreadable.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty()
            && self.library_problems.is_empty()
            && self.untracked.is_empty()
            && self.errors.is_empty()
    }
}

/// Read-only: re-hashes every library file recorded in library.db, then confirms each media
/// file under `source` has a verified copy. Never writes to the source or the library.
pub fn check(source: &Path, library: &Path) -> anyhow::Result<CheckReport> {
    check_with_progress(source, library, &mut |_| {})
}

/// [`check`], calling `progress` with the fraction of files hashed so far.
pub fn check_with_progress(
    source: &Path,
    library: &Path,
    progress: &mut dyn FnMut(f32),
) -> anyhow::Result<CheckReport> {
    let library = std::fs::canonicalize(library)
        .with_context(|| format!("library {} not found", library.display()))?;
    let db_path = library.join("library.db");
    if !db_path.is_file() {
        bail!("{} has no library.db; run import first", library.display());
    }
    let db = Db::open_read_only(&db_path)?;
    let mut report = CheckReport::default();

    let recorded = db.recorded_files()?;
    let known_paths: HashSet<_> = recorded
        .iter()
        .map(|(_, relative)| library.join(relative))
        .collect();

    let mut scan = ImportReport::default();
    let mut library_files = Vec::new();
    walk(&library, &db_path, &mut library_files, &mut scan);
    report.errors.extend(scan.errors);
    report.untracked = library_files
        .into_iter()
        .filter(|path| !known_paths.contains(path))
        .collect();

    let mut scan = ImportReport::default();
    let mut source_files = Vec::new();
    let source = std::fs::canonicalize(source)
        .with_context(|| format!("source {} not found", source.display()))?;
    walk(&source, &library, &mut source_files, &mut scan);
    report.ignored = scan.ignored;
    report.errors.extend(scan.errors);

    // Walk both trees first so the hashing below can report progress against a known total.
    let total = (recorded.len() + source_files.len()) as f32;
    let mut hashed = 0;
    let mut verified = HashSet::new();
    for (expected, relative) in recorded {
        progress(hashed as f32 / total);
        hashed += 1;
        match hash::hash_file(&library.join(&relative)) {
            Ok(actual) if actual == expected => {
                report.verified_library_files += 1;
                verified.insert(expected);
            }
            Ok(_) => report
                .library_problems
                .push(format!("{relative}: contents changed since import")),
            Err(error) => report.library_problems.push(format!("{relative}: {error}")),
        }
    }
    for path in source_files {
        progress(hashed as f32 / total);
        hashed += 1;
        match hash::hash_file(&path) {
            Ok(h) if verified.contains(&h) => report.safe.push(path),
            Ok(_) => report.missing.push(path),
            Err(error) => report.errors.push(format!("{}: {error}", path.display())),
        }
    }
    progress(1.0);
    Ok(report)
}
