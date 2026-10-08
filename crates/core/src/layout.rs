use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};

/// Relative destination dir: "YYYY/MM" or "_unsorted".
pub fn dest_dir(taken: Option<NaiveDateTime>) -> PathBuf {
    todo!()
}

/// File stem: "YYYY-MM-DD_HHMMSS_<short6>" or "nodate_<short6>" when there is no date.
pub fn file_stem(taken: Option<NaiveDateTime>, full_hash: &str) -> String {
    todo!()
}

/// Pick a path under `root/dest_dir` named `stem.ext` (ext lowercased). If that path exists,
/// append "-1", "-2", ... to the stem until free. Never returns an existing path.
pub fn unique_path(root: &Path, rel_dir: &Path, stem: &str, ext: &str) -> PathBuf {
    todo!()
}
