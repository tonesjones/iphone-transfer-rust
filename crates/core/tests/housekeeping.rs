#[allow(dead_code)]
mod common;

use photoxfer_core::{
    archive::{archive, verify},
    db::Db,
    housekeeping::{KEEP, prune_recovery_sets, prune_snapshots, remove_partials},
    import::import_folder,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn stem(day: usize) -> String {
    format!("library-200001{day:02}T000000.000000000Z")
}

fn write(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"preserved bytes").unwrap();
}

fn snapshots(dir: &Path, count: usize) -> Vec<PathBuf> {
    (1..=count)
        .map(|day| {
            let path = dir.join(format!("{}.db", stem(day)));
            write(&path);
            path
        })
        .collect()
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn retains_ten_newest_snapshots_and_unrelated_files() {
    let temp = tempfile::tempdir().unwrap();
    let paths = snapshots(temp.path(), 12);
    let unrelated = ["library-notes.db", "library.db", "other.db"];
    for name in unrelated {
        write(&temp.path().join(name));
    }
    assert_eq!(prune_snapshots(temp.path(), KEEP).unwrap(), paths[..2]);
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(path.exists(), index >= 2);
    }
    for name in unrelated {
        assert!(temp.path().join(name).is_file());
    }
    assert_eq!(files(temp.path()).len(), 13);
}

#[test]
fn rejects_malformed_snapshot_names_and_directories() {
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "library-20000101T000000.Z.db",
        "library-20000101T000000.123Z.db.extra",
        "library-20000101t000000.123Z.db",
        "library-20000101T00000.123Z.db",
        "library-20000101T000000x123Z.db",
        "library-20000101T000000.12xZ.db",
        "library-20000101T000000.123z.db",
        "library-2000010xT000000.123Z.db",
    ] {
        write(&temp.path().join(name));
    }
    fs::create_dir(temp.path().join(format!("{}.db", stem(1)))).unwrap();
    let before = files(temp.path());
    assert!(prune_snapshots(temp.path(), 0).unwrap().is_empty());
    assert_eq!(files(temp.path()), before);
    assert!(
        prune_snapshots(&temp.path().join("missing"), KEEP)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn prunes_complete_recovery_pairs_and_preserves_unpaired_files() {
    let temp = tempfile::tempdir().unwrap();
    let backups = temp.path().join("catalog-backups");
    let manifests = temp.path().join("manifests");
    let paths = snapshots(&backups, 12);
    let partners: Vec<_> = (1..=12)
        .map(|day| {
            let path = manifests.join(format!("{}.tsv", stem(day)));
            write(&path);
            path
        })
        .collect();
    let lone_db = backups.join("library-19990101T000000.000000000Z.db");
    let lone_manifest = manifests.join("library-19990102T000000.000000000Z.tsv");
    write(&lone_db);
    write(&lone_manifest);
    assert_eq!(
        prune_recovery_sets(temp.path(), KEEP).unwrap(),
        vec![
            paths[0].clone(),
            partners[0].clone(),
            paths[1].clone(),
            partners[1].clone()
        ]
    );
    for index in 0..12 {
        assert_eq!(paths[index].exists(), index >= 2);
        assert_eq!(partners[index].exists(), index >= 2);
    }
    assert!(lone_db.is_file());
    assert!(lone_manifest.is_file());
    assert_eq!(files(&backups).len(), 11);
    assert_eq!(files(&manifests).len(), 11);
}

#[test]
fn removes_only_hidden_partial_regular_files_recursively() {
    let temp = tempfile::tempdir().unwrap();
    let partial = temp.path().join("a/.x.jpg.1.2.partial");
    write(&partial);
    let kept = [
        "a/x.jpg".to_string(),
        "library.db".to_string(),
        ".photoxfer.lock".to_string(),
        format!(".catalog-backups/{}.db", stem(1)),
        "a/x.partial".to_string(),
    ];
    for name in &kept {
        write(&temp.path().join(name));
    }
    let directory = temp.path().join("a/.directory.partial");
    fs::create_dir(&directory).unwrap();
    assert_eq!(remove_partials(temp.path()).unwrap(), vec![partial.clone()]);
    assert!(!partial.exists());
    for name in kept {
        assert_eq!(
            fs::read(temp.path().join(name)).unwrap(),
            b"preserved bytes"
        );
    }
    assert!(directory.is_dir());
}

#[test]
fn failed_import_preserves_snapshots_and_partial() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    let report = import_folder(&inbox, &library).unwrap();
    fs::write(&report.saved[0], b"damaged").unwrap();
    let backups = library.join(".catalog-backups");
    for path in files(&backups) {
        fs::remove_file(path).unwrap();
    }
    let paths = snapshots(&backups, 11);
    let partial = library.join(".leftover.1.2.partial");
    write(&partial);
    assert_eq!(import_folder(&inbox, &library).unwrap().failed, 1);
    assert_eq!(files(&backups), paths);
    assert_eq!(fs::read(partial).unwrap(), b"preserved bytes");
    for path in files(&backups) {
        assert_eq!(fs::read(path).unwrap(), b"preserved bytes");
    }
}

#[test]
fn successful_import_retains_new_snapshot_and_cleans_partial() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let backups = library.join(".catalog-backups");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    let old = snapshots(&backups, 11);
    let partial = library.join(".leftover.1.2.partial");
    write(&partial);
    let report = import_folder(&inbox, &library).unwrap();
    assert_eq!((report.failed, report.copied), (0, 1));
    let remaining = files(&backups);
    assert_eq!(remaining.len(), KEEP);
    assert_eq!(&remaining[..KEEP - 1], &old[2..]);
    let newest = remaining.last().unwrap();
    assert!(!old.contains(newest));
    let db = Db::open_read_only(newest).unwrap();
    let count: i64 = db
        .conn
        .query_row("SELECT count(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert!(!partial.exists());
}

#[test]
fn successful_archive_retains_new_recovery_set_and_cleans_partial() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let destination = temp.path().join("archive");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let backups = destination.join("catalog-backups");
    let manifests = destination.join("manifests");
    let old = snapshots(&backups, 11);
    for day in 1..=11 {
        write(&manifests.join(format!("{}.tsv", stem(day))));
    }
    let partial = destination.join("nested/.leftover.1.2.partial");
    write(&partial);
    let report = archive(&library, &destination).unwrap();
    let remaining = files(&backups);
    assert_eq!(remaining.len(), KEEP);
    assert_eq!(&remaining[..KEEP - 1], &old[2..]);
    assert_eq!(files(&manifests).len(), KEEP);
    assert_eq!(
        fs::canonicalize(files(&manifests).last().unwrap()).unwrap(),
        report.manifest
    );
    assert_eq!(verify(&destination, &report.manifest).unwrap(), 2);
    assert!(!partial.exists());
}
