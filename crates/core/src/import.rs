use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub found: u64,
    pub copied: u64,
    pub skipped: u64,
    pub failed: u64,
    pub errors: Vec<String>,
}

/// Import every file under `inbox` (recursive) into `library` (db at library/library.db).
pub fn import_folder(inbox: &Path, library: &Path) -> anyhow::Result<ImportReport> {
    todo!()
}

/// Copy src to dest via a temp file in dest's dir, fsync, re-hash the copy, and only rename into
/// place if the hash equals `expected_hash`. On mismatch remove the temp and return Err.
pub fn copy_verified(src: &Path, dest: &Path, expected_hash: &str) -> anyhow::Result<()> {
    todo!()
}
