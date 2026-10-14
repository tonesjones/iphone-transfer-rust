use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::{
    db::Db,
    hash,
    import::{ImportReport, walk},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub hash: String,
    pub old: String,
    pub new: String,
}

/// Pair each recorded file that is gone with an unrecorded library file of identical contents.
/// Read-only. Unrecorded files are only hashed when something recorded is missing.
pub fn detect(library: &Path, recorded: &[(String, String)], untracked: &[PathBuf]) -> Vec<Rename> {
    let missing: Vec<_> = recorded
        .iter()
        .filter(|(_, relative)| !matches!(library.join(relative).try_exists(), Ok(true)))
        .collect();
    if missing.is_empty() || untracked.is_empty() {
        return Vec::new();
    }
    let mut by_hash: HashMap<String, Vec<&PathBuf>> = HashMap::new();
    for path in untracked {
        if let Ok(h) = hash::hash_file(path) {
            by_hash.entry(h).or_default().push(path);
        }
    }
    let mut renames = Vec::new();
    for (expected, old) in missing {
        let Some(paths) = by_hash.get_mut(expected) else {
            continue;
        };
        paths.sort();
        if let Some(path) = paths.first().copied() {
            let new = path
                .strip_prefix(library)
                .unwrap_or(path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            paths.remove(0);
            renames.push(Rename {
                hash: expected.clone(),
                old: old.clone(),
                new,
            });
        }
    }
    renames
}

/// Detect renames in `library` against the catalog in `db`.
pub fn scan(library: &Path, db: &Db) -> anyhow::Result<Vec<Rename>> {
    let recorded = db.recorded_files()?;
    let known: HashSet<_> = recorded.iter().map(|(_, r)| library.join(r)).collect();
    let mut files = Vec::new();
    walk(
        library,
        &library.join("library.db"),
        &mut files,
        &mut ImportReport::default(),
    );
    files.retain(|path| !known.contains(path));
    Ok(detect(library, &recorded, &files))
}

/// Record renames in the catalog and keep the new file name searchable. Bytes are not touched.
pub fn apply(db: &Db, renames: &[Rename]) -> anyhow::Result<()> {
    let tx = db.conn.unchecked_transaction()?;
    for rename in renames {
        for table in ["assets", "sidecars"] {
            tx.execute(
                &format!(
                    "UPDATE {table} SET library_path=?1 WHERE library_path=?2 AND {}=?3",
                    if table == "assets" {
                        "hash"
                    } else {
                        "sidecar_hash"
                    }
                ),
                params![rename.new, rename.old, rename.hash],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO renames(hash,old_path,new_path) VALUES(?1,?2,?3)",
            params![rename.hash, rename.old, rename.new],
        )?;
        let name = rename.new.rsplit('/').next().unwrap_or(&rename.new);
        tx.execute(
            "INSERT OR IGNORE INTO source_files(hash,original_name,source_path) VALUES(?1,?2,?3)",
            params![
                rename.hash,
                name,
                format!("renamed in library: {}", rename.new)
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Detect and record renames; returns what changed.
pub fn reconcile(library: &Path, db: &Db) -> anyhow::Result<Vec<Rename>> {
    let renames = scan(library, db)?;
    apply(db, &renames)?;
    Ok(renames)
}
