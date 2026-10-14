use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::{
    db::{AssetRow, Db},
    hash, import,
    metadata::{self, MediaKind},
    pairing,
};

#[derive(Debug, Default)]
pub struct ReindexReport {
    pub restored_from: Option<PathBuf>,
    pub adopted: Vec<String>,
    pub duplicates: Vec<(String, String)>,
    pub errors: Vec<String>,
}

pub fn reindex(library: &Path) -> anyhow::Result<ReindexReport> {
    let library = fs::canonicalize(library)?;
    let _lock = import::lock_library(&library)?;
    let db_path = library.join("library.db");
    let mut report = ReindexReport::default();
    if !db_path.try_exists()? {
        report.restored_from = restore_snapshot(&library, &db_path)?;
    }
    let db = Db::open(&db_path)?;

    let recorded: HashMap<_, _> = db
        .recorded_files()?
        .into_iter()
        .map(|(hash, path)| (library.join(path), hash))
        .collect();
    let mut scan = import::ImportReport::default();
    let mut files = Vec::new();
    import::walk(&library, &db_path, &mut files, &mut scan);
    report.errors.extend(scan.errors);
    files.sort();
    let unrecorded: HashSet<_> = files
        .iter()
        .filter(|path| !recorded.contains_key(*path))
        .cloned()
        .collect();
    let mut hashes = recorded;
    for path in files.iter().filter(|path| unrecorded.contains(*path)) {
        match hash::hash_file(path) {
            Ok(hash) => {
                hashes.insert(path.clone(), hash);
            }
            Err(error) => report
                .errors
                .push(format!("{}: {error}", relative(&library, path)?)),
        }
    }
    if !report.errors.is_empty() || unrecorded.is_empty() {
        return Ok(report);
    }

    db.snapshot(&library.join(".catalog-backups"))?;
    let transaction = db.conn.unchecked_transaction()?;
    // Keep recorded partners in groups so a newly found MOV or AAE can attach to them.
    for group in pairing::group_files(&files) {
        let members: Vec<_> = std::iter::once(&group.primary)
            .chain(group.live_mov.iter())
            .chain(group.sidecar.iter())
            .collect();
        if !members.iter().any(|path| unrecorded.contains(*path)) {
            continue;
        }
        let taken = metadata::capture_time(&group.primary);
        let mut photo = None;
        let mut mov = None;
        let mut adopted_media = false;
        for (index, path) in members.into_iter().enumerate() {
            let file_hash = &hashes[path];
            let kind = metadata::kind_for(path);
            if unrecorded.contains(path) {
                let relative = relative(&library, path)?;
                if let Some(other) = db.stored_path(file_hash)? {
                    report.duplicates.push((relative, other));
                } else {
                    if let (MediaKind::Sidecar, Some(photo)) = (kind, photo.as_deref()) {
                        db.link_sidecar(photo, file_hash, &relative)?;
                    } else {
                        let asset_kind = match kind {
                            MediaKind::Video => "video",
                            MediaKind::Sidecar => "sidecar",
                            MediaKind::Photo if index == 0 && group.live_mov.is_some() => "live",
                            _ => "photo",
                        };
                        db.insert_asset(&AssetRow {
                            hash: file_hash.clone(),
                            kind: asset_kind.into(),
                            taken_at: taken.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
                            library_path: relative.clone(),
                        })?;
                        adopted_media |= kind != MediaKind::Sidecar;
                    }
                    report.adopted.push(relative);
                }
            }
            if kind != MediaKind::Sidecar && db.has_asset(file_hash)? {
                if index == 0 && kind == MediaKind::Photo {
                    photo = Some(file_hash.clone());
                } else if group.live_mov.as_ref() == Some(path) {
                    mov = Some(file_hash.clone());
                }
            }
        }
        if let (true, Some(photo), Some(mov)) = (adopted_media, photo, mov) {
            db.link_live_pair(&photo, &mov)?;
        }
    }
    transaction.commit()?;
    Ok(report)
}

fn relative(library: &Path, path: &Path) -> anyhow::Result<String> {
    Ok(path
        .strip_prefix(library)?
        .to_string_lossy()
        .replace('\\', "/"))
}

struct TempGuard(PathBuf);

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn restore_snapshot(library: &Path, db_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let entries = match fs::read_dir(library.join(".catalog-backups")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_file() && name.starts_with("library-") && name.ends_with(".db") {
            snapshots.push(entry.path());
        }
    }
    snapshots.sort();
    for snapshot in snapshots.into_iter().rev() {
        let usable = (|| -> rusqlite::Result<bool> {
            let db = Db::open_read_only(&snapshot)?;
            let results: Vec<String> = db
                .conn
                .prepare("PRAGMA integrity_check")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            // An empty file left by a failed snapshot also passes integrity_check.
            let has_catalog: bool = db.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='assets')",
                [],
                |row| row.get(0),
            )?;
            Ok(results == ["ok"] && has_catalog)
        })();
        if !matches!(usable, Ok(true)) {
            continue;
        }
        let temp_path = library.join(format!(
            ".library.db.{}.{}.partial",
            std::process::id(),
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ")
        ));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let temp = TempGuard(temp_path);
        let copied = (|| -> std::io::Result<()> {
            let mut input = fs::File::open(&snapshot)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()
        })();
        drop(output);
        copied.with_context(|| format!("copying snapshot {}", snapshot.display()))?;
        if db_path.try_exists()? {
            bail!("catalog already exists: {}", db_path.display());
        }
        publish_catalog(&temp.0, db_path)?;
        return Ok(Some(snapshot));
    }
    Ok(None)
}

/// Publishes without replacing an existing catalog; the TempGuard then removes the temp name.
fn publish_catalog(temp: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(temp, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_never_replaces_an_existing_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join(".restore.partial");
        let destination = temp.path().join("library.db");
        fs::write(&source, b"snapshot").unwrap();
        fs::write(&destination, b"existing catalog").unwrap();
        assert!(publish_catalog(&source, &destination).is_err());
        assert_eq!(fs::read(source).unwrap(), b"snapshot");
        assert_eq!(fs::read(destination).unwrap(), b"existing catalog");
    }
}
