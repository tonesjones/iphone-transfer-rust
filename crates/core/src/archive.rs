use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::{
    check,
    db::Db,
    hash,
    import::{copy_verified, lock_library},
};
use anyhow::{Context, bail};

#[derive(Debug)]
pub struct ArchiveReport {
    pub copied: usize,
    pub verified: usize,
    pub manifest: PathBuf,
}

fn relative_path(text: &str) -> anyhow::Result<&Path> {
    let path = Path::new(text);
    if text.contains(['\t', '\n', '\r'])
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("invalid relative archive path: {text:?}");
    }
    Ok(path)
}

/// Ordinary independent copies; never mirror deletions or overwrite a conflict.
pub fn archive(library: &Path, destination: &Path) -> anyhow::Result<ArchiveReport> {
    let library = fs::canonicalize(library)?;
    fs::create_dir_all(destination)?;
    let destination = fs::canonicalize(destination)?;
    if destination.starts_with(&library) || library.starts_with(&destination) {
        bail!("archive and working library must be separate, non-nested folders");
    }
    let _lock = lock_library(&library)?;
    let report = check::check(&library, &library)?;
    if !report.is_clean() {
        bail!(
            "library verification failed; run check and resolve its reported problems before archiving"
        );
    }
    let db = Db::open_read_only(&library.join("library.db"))?;
    let mut records = db.recorded_files()?;
    records.sort_by(|a, b| a.1.cmp(&b.1));
    let mut copied = 0;
    let mut manifest = String::from("photoxfer-blake3-v1\n");
    for (expected, relative) in &records {
        let path = relative_path(relative)?;
        let source = library.join(path);
        // Recheck against the catalog, including changes by programs outside photoxfer.
        if hash::hash_file(&source)? != *expected {
            bail!("library file changed during archive: {relative}");
        }
        let target = destination.join(path);
        if target.try_exists()? {
            if hash::hash_file(&target)? != *expected {
                bail!(
                    "archive conflict preserved at {}; resolve it before retrying",
                    target.display()
                );
            }
        } else {
            copy_verified(&source, &target, expected)?;
            copied += 1;
        }
        let size = fs::metadata(&source)?.len();
        manifest.push_str(&format!("{expected}\t{size}\t{relative}\n"));
    }
    let snapshot = db.snapshot(&destination.join("catalog-backups"))?;
    let snapshot_db = Db::open_read_only(&snapshot)?;
    let integrity: String = snapshot_db
        .conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        bail!("database snapshot failed integrity check: {integrity}");
    }
    drop(snapshot_db);
    let relative = snapshot
        .strip_prefix(&destination)?
        .to_string_lossy()
        .replace('\\', "/");
    let snapshot_hash = hash::hash_file(&snapshot)?;
    manifest.push_str(&format!(
        "{snapshot_hash}\t{}\t{relative}\n",
        fs::metadata(&snapshot)?.len()
    ));
    manifest.push_str(&format!("complete\t{}\n", records.len() + 1));
    let manifests = destination.join("manifests");
    fs::create_dir_all(&manifests)?;
    let manifest_path = manifests.join(format!(
        "{}.tsv",
        snapshot.file_stem().unwrap().to_string_lossy()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&manifest_path)?;
    file.write_all(manifest.as_bytes())?;
    file.sync_all()?;
    drop(file);
    verify(&destination, &manifest_path)?;
    Ok(ArchiveReport {
        copied,
        verified: records.len(),
        manifest: manifest_path,
    })
}

/// Verify a cloud-restored batch without writing to it. Manifest paths are relative to root.
pub fn verify(root: &Path, manifest: &Path) -> anyhow::Result<usize> {
    let root = fs::canonicalize(root)?;
    let text = fs::read_to_string(manifest)?;
    let mut lines = text.lines();
    if lines.next() != Some("photoxfer-blake3-v1") {
        bail!("unsupported archive manifest");
    }
    let mut records: Vec<_> = lines.collect();
    let expected_count: usize = records
        .pop()
        .and_then(|line| line.strip_prefix("complete\t"))
        .context("manifest is incomplete: missing completion record")?
        .parse()?;
    if records.len() != expected_count {
        bail!("manifest is incomplete: file count differs");
    }
    let mut count = 0;
    for line in records {
        let fields: Vec<_> = line.splitn(3, '\t').collect();
        if fields.len() != 3 {
            bail!("invalid manifest record");
        }
        let expected_size: u64 = fields[1].parse()?;
        let relative = relative_path(fields[2])?;
        let path = fs::canonicalize(root.join(relative))
            .with_context(|| format!("missing archive file: {}", fields[2]))?;
        if !path.starts_with(&root) {
            bail!("archive path leaves restore folder");
        }
        if fs::metadata(&path)?.len() != expected_size || hash::hash_file(&path)? != fields[0] {
            bail!("archive verification failed: {}", fields[2]);
        }
        count += 1;
    }
    if count == 0 {
        bail!("manifest contains no files");
    }
    Ok(count)
}
