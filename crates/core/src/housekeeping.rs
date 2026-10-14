use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const KEEP: usize = 10;

fn real_directory(dir: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) => Ok(!metadata.file_type().is_symlink() && metadata.is_dir()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn valid_partial(name: &[u8]) -> bool {
    let Some(name) = name
        .strip_prefix(b".")
        .and_then(|name| name.strip_suffix(b".partial"))
    else {
        return false;
    };
    let mut parts = name.rsplitn(3, |byte| *byte == b'.');
    let digits = |part: Option<&[u8]>| {
        part.is_some_and(|part| !part.is_empty() && part.iter().all(u8::is_ascii_digit))
    };
    digits(parts.next())
        && digits(parts.next())
        && parts.next().is_some_and(|name| !name.is_empty())
}

fn valid_stem(stem: &str) -> bool {
    let Some(timestamp) = stem.strip_prefix("library-") else {
        return false;
    };
    let bytes = timestamp.as_bytes();
    bytes.len() == 26
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'.'
        && bytes[16..bytes.len() - 1].iter().all(u8::is_ascii_digit)
        && bytes[bytes.len() - 1] == b'Z'
}

fn snapshots(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !real_directory(dir)? {
        return Ok(Vec::new());
    }
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

pub fn prune_snapshots(dir: &Path, keep: usize, current: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let paths: Vec<_> = snapshots(dir)?
        .into_iter()
        .filter(|path| path.file_name() < current.file_name())
        .collect();
    let mut deleted = Vec::new();
    for path in paths
        .iter()
        .take(paths.len().saturating_sub(keep.saturating_sub(1)))
    {
        fs::remove_file(path)?;
        deleted.push(path.clone());
    }
    Ok(deleted)
}

pub fn prune_recovery_sets(
    archive: &Path,
    keep: usize,
    current: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    if !real_directory(&archive.join("catalog-backups"))?
        || !real_directory(&archive.join("manifests"))?
    {
        return Ok(Vec::new());
    }
    let mut sets = Vec::new();
    for snapshot in snapshots(&archive.join("catalog-backups"))? {
        if snapshot.file_name() >= current.file_name() {
            continue;
        }
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
    for (snapshot, manifest) in sets
        .iter()
        .take(sets.len().saturating_sub(keep.saturating_sub(1)))
    {
        fs::remove_file(manifest)?;
        deleted.push(manifest.clone());
        fs::remove_file(snapshot)?;
        deleted.push(snapshot.clone());
    }
    Ok(deleted)
}

pub fn remove_partials(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !real_directory(root)? {
        return Ok(Vec::new());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(dir) = directories.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                directories.push(entry.path());
            } else if kind.is_file() {
                let name = entry.file_name();
                let bytes = name.as_encoded_bytes();
                if valid_partial(bytes) {
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
