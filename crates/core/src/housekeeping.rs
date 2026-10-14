use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const KEEP: usize = 10;

fn valid_stem(stem: &str) -> bool {
    let Some(timestamp) = stem.strip_prefix("library-") else {
        return false;
    };
    let bytes = timestamp.as_bytes();
    bytes.len() >= 18
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'.'
        && bytes[16..bytes.len() - 1].iter().all(u8::is_ascii_digit)
        && bytes[bytes.len() - 1] == b'Z'
}

fn snapshots(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".db"))
                .is_some_and(valid_stem)
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn prune_snapshots(dir: &Path, keep: usize) -> anyhow::Result<Vec<PathBuf>> {
    let paths = snapshots(dir)?;
    let mut deleted = Vec::new();
    for path in paths.iter().take(paths.len().saturating_sub(keep)) {
        fs::remove_file(path)?;
        deleted.push(path.clone());
    }
    Ok(deleted)
}

pub fn prune_recovery_sets(archive: &Path, keep: usize) -> anyhow::Result<Vec<PathBuf>> {
    let mut sets = Vec::new();
    for snapshot in snapshots(&archive.join("catalog-backups"))? {
        let manifest = archive.join("manifests").join(format!(
            "{}.tsv",
            snapshot.file_stem().unwrap().to_str().unwrap()
        ));
        match fs::symlink_metadata(&manifest) {
            Ok(metadata) if metadata.is_file() => sets.push((snapshot, manifest)),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut deleted = Vec::new();
    for (snapshot, manifest) in sets.iter().take(sets.len().saturating_sub(keep)) {
        fs::remove_file(snapshot)?;
        deleted.push(snapshot.clone());
        fs::remove_file(manifest)?;
        deleted.push(manifest.clone());
    }
    Ok(deleted)
}

pub fn remove_partials(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !fs::symlink_metadata(root)?.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(dir) = directories.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.push(entry.path());
            } else if kind.is_file() {
                let name = entry.file_name();
                let bytes = name.as_encoded_bytes();
                if bytes.starts_with(b".") && bytes.ends_with(b".partial") {
                    paths.push(entry.path());
                }
            }
        }
    }
    paths.sort();
    for path in &paths {
        fs::remove_file(path)?;
    }
    Ok(paths)
}
