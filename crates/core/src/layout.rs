use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};

/// Relative destination dir: "YYYY/MM" or "_unsorted".
pub fn dest_dir(taken: Option<NaiveDateTime>) -> PathBuf {
    taken
        .map(|dt| PathBuf::from(dt.format("%Y/%m").to_string()))
        .unwrap_or_else(|| PathBuf::from("_unsorted"))
}

/// File stem: "YYYY-MM-DD_HHMMSS_<short6>" or "nodate_<short6>" when there is no date.
pub fn file_stem(taken: Option<NaiveDateTime>, full_hash: &str) -> String {
    match taken {
        Some(dt) => format!(
            "{}_{}",
            dt.format("%Y-%m-%d_%H%M%S"),
            crate::hash::short_hash(full_hash)
        ),
        None => format!("nodate_{}", crate::hash::short_hash(full_hash)),
    }
}

/// Pick a path under `root/dest_dir` named `stem.ext` (ext lowercased). If that path exists,
/// append "-1", "-2", ... to the stem until free. Never returns an existing path.
pub fn unique_path(root: &Path, rel_dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let directory = root.join(rel_dir);
    let extension = ext.to_ascii_lowercase();
    let mut candidate = directory.join(format!("{stem}.{extension}"));
    let mut suffix = 1u64;
    while candidate.exists() {
        candidate = directory.join(format!("{stem}-{suffix}.{extension}"));
        suffix += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 7)
            .and_then(|d| d.and_hms_opt(14, 30, 12))
            .expect("valid date")
    }

    #[test]
    fn date_layout_and_stem() {
        assert_eq!(dest_dir(Some(date())), PathBuf::from("2026/10"));
        assert_eq!(dest_dir(None), PathBuf::from("_unsorted"));
        assert_eq!(
            file_stem(Some(date()), "a1b2c3deadbeef"),
            "2026-10-07_143012_a1b2c3"
        );
        assert_eq!(file_stem(None, "a1b2c3deadbeef"), "nodate_a1b2c3");
    }

    #[test]
    fn paths_try_two_collisions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rel = Path::new("2026/10");
        assert_eq!(
            unique_path(temp.path(), rel, "x", "JPG"),
            temp.path().join(rel).join("x.jpg")
        );
        assert!(!temp.path().join(rel).exists());
        std::fs::create_dir_all(temp.path().join(rel)).expect("directory");
        std::fs::write(temp.path().join(rel).join("x.jpg"), []).expect("file");
        let one = unique_path(temp.path(), rel, "x", "JPG");
        assert_eq!(one, temp.path().join(rel).join("x-1.jpg"));
        assert!(!one.exists());
        std::fs::write(&one, []).expect("file");
        assert_eq!(
            unique_path(temp.path(), rel, "x", "JPG"),
            temp.path().join(rel).join("x-2.jpg")
        );
        assert!(!temp.path().join(rel).join("x-2.jpg").exists());
        assert_eq!(
            std::fs::read_dir(temp.path().join(rel))
                .expect("entries")
                .count(),
            2
        );
    }
}
