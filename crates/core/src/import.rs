use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    db::{AssetRow, Db},
    hash, layout,
    metadata::{self, MediaKind},
    pairing,
};
use anyhow::{Context, bail};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub found: u64,
    pub copied: u64,
    pub skipped: u64,
    pub failed: u64,
    pub errors: Vec<String>,
}

fn failed(report: &mut ImportReport, path: &Path, error: impl std::fmt::Display) {
    report.failed += 1;
    report.errors.push(format!("{}: {error}", path.display()));
}

fn walk(dir: &Path, library: &Path, files: &mut Vec<PathBuf>, report: &mut ImportReport) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            failed(report, dir, error);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failed(report, dir, error);
                continue;
            }
        };
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') || path == library {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&path, library, files, report),
            Ok(kind) if kind.is_file() => files.push(path),
            Ok(_) => {} // Includes symlinks; never follow them.
            Err(error) => failed(report, &path, error),
        }
    }
}

/// Import every file under `inbox` (recursive) into `library` (db at library/library.db).
pub fn import_folder(inbox: &Path, library: &Path) -> anyhow::Result<ImportReport> {
    fs::create_dir_all(library)?;
    let library = fs::canonicalize(library)?;
    let db = Db::open(&library.join("library.db"))?;
    let id = db.begin_import(&inbox.display().to_string())?;
    let mut report = ImportReport::default();
    let mut files = Vec::new();
    match fs::canonicalize(inbox) {
        Ok(inbox) if inbox != library => walk(&inbox, &library, &mut files, &mut report),
        Ok(_) => {}
        Err(error) => failed(&mut report, inbox, error),
    }
    report.found = files.len() as u64;
    for group in pairing::group_files(&files) {
        let taken = metadata::capture_time(&group.primary);
        let primary_hash = hash::hash_file(&group.primary);
        let stem = primary_hash
            .as_ref()
            .ok()
            .map(|h| layout::file_stem(taken, h));
        let mut photo = None;
        let mut mov = None;
        for (index, src) in std::iter::once(&group.primary)
            .chain(group.live_mov.iter())
            .chain(group.sidecar.iter())
            .enumerate()
        {
            let file_hash = if index == 0 {
                match &primary_hash {
                    Ok(h) => h.clone(),
                    Err(error) => {
                        failed(&mut report, src, error);
                        continue;
                    }
                }
            } else {
                match hash::hash_file(src) {
                    Ok(h) => h,
                    Err(error) => {
                        failed(&mut report, src, error);
                        continue;
                    }
                }
            };
            let kind = metadata::kind_for(src);
            let sidecar = kind == MediaKind::Sidecar;
            // A sidecar with no primary in the library is stored as its own asset, since
            // sidecars.asset_hash must reference an asset.
            let exists = if sidecar {
                db.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sidecars WHERE sidecar_hash=?1)",
                    [&file_hash],
                    |r| r.get::<_, bool>(0),
                )? || db.has_asset(&file_hash)?
            } else {
                db.has_asset(&file_hash)?
            };
            if exists {
                report.skipped += 1;
            } else {
                // Pair files share the primary's date and stem; without a primary hash they
                // fall back to their own.
                let file_taken = if stem.is_some() {
                    taken
                } else {
                    metadata::capture_time(src)
                };
                let own_stem = layout::file_stem(file_taken, &file_hash);
                let file_stem = stem.as_deref().unwrap_or(&own_stem);
                let ext = src
                    .extension()
                    .map(|e| e.to_string_lossy())
                    .unwrap_or_default();
                let dest =
                    layout::unique_path(&library, &layout::dest_dir(file_taken), file_stem, &ext);
                if let Err(error) = copy_verified(src, &dest, &file_hash) {
                    failed(&mut report, src, format!("{error:#}"));
                    continue;
                }
                let relative = dest
                    .strip_prefix(&library)?
                    .to_string_lossy()
                    .replace('\\', "/");
                let inserted = if let (true, Some(photo)) = (sidecar, photo.as_deref()) {
                    db.link_sidecar(photo, &file_hash, &relative)
                } else {
                    let kind = match kind {
                        MediaKind::Video => "video",
                        MediaKind::Sidecar => "sidecar",
                        MediaKind::Photo if index == 0 && group.live_mov.is_some() => "live",
                        _ => "photo",
                    };
                    db.insert_asset(&AssetRow {
                        hash: file_hash.clone(),
                        kind: kind.into(),
                        taken_at: file_taken.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
                        library_path: relative,
                    })
                };
                if let Err(error) = inserted {
                    fs::remove_file(&dest).with_context(|| {
                        format!(
                            "DB insert failed ({error}); removing {} also failed",
                            dest.display()
                        )
                    })?;
                    return Err(error.into());
                }
                report.copied += 1;
            }
            if !sidecar {
                if index == 0 && kind == MediaKind::Photo {
                    photo = Some(file_hash);
                } else if group.live_mov.as_ref() == Some(src) {
                    mov = Some(file_hash);
                }
            }
        }
        if let (Some(photo), Some(mov)) = (photo, mov) {
            db.link_live_pair(&photo, &mov)?;
        }
    }
    db.finish_import(
        id,
        report.copied,
        report.skipped,
        report.failed,
        &report.errors.join("\n"),
    )?;
    Ok(report)
}

struct TempGuard(PathBuf);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Stream, sync, and independently re-hash a same-directory temporary copy before publishing.
pub fn copy_verified(src: &Path, dest: &Path, expected_hash: &str) -> anyhow::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = dest.file_name().context("destination has no file name")?;
    let (temp, mut output) = loop {
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(
            ".{}.{}.partial",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = parent.join(temp_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (TempGuard(path), file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    // Close both handles before the guard runs, including on errors (important on Windows).
    let copied = (|| -> std::io::Result<()> {
        let mut input = File::open(src)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()
    })();
    drop(output);
    copied?;
    let actual = hash::hash_file(&temp.0)?;
    if actual != expected_hash {
        bail!("hash mismatch: expected {expected_hash}, copied {actual}");
    }
    match fs::hard_link(&temp.0, dest) {
        Ok(()) => {
            if let Err(error) = fs::remove_file(&temp.0) {
                fs::remove_file(dest)
                    .context("failed to roll back destination after temporary cleanup failed")?;
                return Err(error.into());
            }
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(error.into()),
        Err(_) => {
            // Tiny check-to-rename race on filesystems without hard links: another writer
            // can create dest here; platforms whose rename overwrites may replace it.
            if dest.try_exists()? {
                bail!("destination already exists: {}", dest.display());
            }
            fs::rename(&temp.0, dest)?;
        }
    }
    Ok(())
}
