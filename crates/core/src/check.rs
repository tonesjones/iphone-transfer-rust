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

/// Read-only: re-hashes every library file recorded in library.db, then confirms each media
/// file under `source` has a verified copy. Never writes to the source or the library.
pub fn check(source: &Path, library: &Path) -> anyhow::Result<CheckReport> {
    let library = std::fs::canonicalize(library)
        .with_context(|| format!("library {} not found", library.display()))?;
    let db_path = library.join("library.db");
    if !db_path.is_file() {
        bail!("{} has no library.db; run import first", library.display());
    }
    let db = Db::open(&db_path)?;
    let mut report = CheckReport::default();

    let mut recorded: Vec<(String, String)> = Vec::new();
    for sql in [
        "SELECT hash, library_path FROM assets",
        "SELECT sidecar_hash, library_path FROM sidecars",
    ] {
        let mut statement = db.conn.prepare(sql)?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        for row in rows {
            recorded.push(row?);
        }
    }
    let mut verified = HashSet::new();
    let mut known_paths = HashSet::new();
    for (expected, relative) in recorded {
        let path = library.join(&relative);
        known_paths.insert(path.clone());
        match hash::hash_file(&path) {
            Ok(actual) if actual == expected => {
                verified.insert(expected);
            }
            Ok(_) => report
                .library_problems
                .push(format!("{relative}: contents changed since import")),
            Err(error) => report.library_problems.push(format!("{relative}: {error}")),
        }
    }

    let mut scan = ImportReport::default();
    let mut library_files = Vec::new();
    walk(&library, &db_path, &mut library_files, &mut scan);
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
    report.errors = scan.errors;
    for path in source_files {
        match hash::hash_file(&path) {
            Ok(h) if verified.contains(&h) => report.safe.push(path),
            Ok(_) => report.missing.push(path),
            Err(error) => report.errors.push(format!("{}: {error}", path.display())),
        }
    }
    Ok(report)
}
