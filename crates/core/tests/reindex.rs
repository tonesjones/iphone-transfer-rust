#[allow(dead_code)]
mod common;

use photoxfer_core::{check, db::Db, hash, import::import_folder, reindex::reindex};
use rusqlite::types::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let library = temp.path().join("library");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("IMG_1.JPG"), common::jpeg_with_date()).unwrap();
    fs::write(
        source.join("IMG_1.MOV"),
        common::mov_with_offset_datetime("2026-10-07T14:30:12-07:00"),
    )
    .unwrap();
    fs::write(source.join("IMG_1.AAE"), b"paired sidecar").unwrap();
    fs::write(source.join("standalone.PNG"), b"undated photo").unwrap();
    fs::write(source.join("orphan.AAE"), b"orphan sidecar").unwrap();
    let report = import_folder(&source, &library).unwrap();
    assert_eq!((report.copied, report.failed), (5, 0));
    (temp, source, library)
}

fn open(library: &Path) -> Db {
    Db::open_read_only(&library.join("library.db")).unwrap()
}

fn rows(db: &Db, query: &str) -> Vec<Vec<Value>> {
    let mut statement = db.conn.prepare(query).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn catalog(db: &Db) -> Vec<Vec<Vec<Value>>> {
    [
        "SELECT * FROM assets ORDER BY hash",
        "SELECT * FROM live_pairs ORDER BY photo_hash",
        "SELECT * FROM sidecars ORDER BY sidecar_hash",
        "SELECT * FROM source_files ORDER BY hash,source_path",
    ]
    .into_iter()
    .map(|query| rows(db, query))
    .collect()
}

fn snapshots(library: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(library.join(".catalog-backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn restores_snapshot_with_assets_links_and_original_names() {
    let (_temp, source, library) = setup();
    let db = open(&library);
    let before = catalog(&db);
    let found = db.find("IMG_1").unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(before[1].len(), 1);
    assert_eq!(before[2].len(), 1);
    drop(db);
    let expected = fs::canonicalize(snapshots(&library).pop().unwrap()).unwrap();
    fs::remove_file(library.join("library.db")).unwrap();

    let report = reindex(&library).unwrap();
    assert_eq!(report.restored_from, Some(expected));
    assert!(report.adopted.is_empty());
    assert!(report.duplicates.is_empty());
    assert!(report.errors.is_empty());
    let db = open(&library);
    assert_eq!(catalog(&db), before);
    assert_eq!(db.find("IMG_1").unwrap(), found);
    assert!(check::check(&source, &library).unwrap().is_clean());
}

#[test]
fn rebuilds_without_snapshots_and_relinks_media() {
    let (_temp, source, library) = setup();
    let db = open(&library);
    let assets = rows(
        &db,
        "SELECT hash,kind,taken_at,library_path FROM assets ORDER BY hash",
    );
    let links = (catalog(&db)[1].clone(), catalog(&db)[2].clone());
    let mut paths: Vec<_> = db
        .recorded_files()
        .unwrap()
        .into_iter()
        .map(|r| r.1)
        .collect();
    paths.sort();
    drop(db);
    fs::remove_file(library.join("library.db")).unwrap();
    fs::remove_dir_all(library.join(".catalog-backups")).unwrap();

    let mut report = reindex(&library).unwrap();
    assert!(report.restored_from.is_none());
    assert!(report.errors.is_empty());
    assert!(report.duplicates.is_empty());
    report.adopted.sort();
    assert_eq!(report.adopted, paths);
    assert!(report.adopted.iter().all(|path| !path.contains('\\')));
    let db = open(&library);
    assert_eq!(
        rows(
            &db,
            "SELECT hash,kind,taken_at,library_path FROM assets ORDER BY hash"
        ),
        assets
    );
    assert_eq!((catalog(&db)[1].clone(), catalog(&db)[2].clone()), links);
    assert!(db.find("IMG_1").unwrap().is_empty());
    assert!(check::check(&source, &library).unwrap().is_clean());
}

#[test]
fn duplicate_is_reported_and_both_copies_are_preserved() {
    let (_temp, _source, library) = setup();
    let db = open(&library);
    let before = catalog(&db);
    let (_, recorded) = db.recorded_files().unwrap().remove(0);
    let original = library.join(&recorded);
    let extra = "_unsorted/extra.JPG";
    let bytes = fs::read(&original).unwrap();
    fs::copy(&original, library.join(extra)).unwrap();
    drop(db);

    let report = reindex(&library).unwrap();
    assert_eq!(report.duplicates, vec![(extra.into(), recorded)]);
    assert!(report.adopted.is_empty());
    assert!(report.errors.is_empty());
    assert_eq!(catalog(&open(&library)), before);
    assert_eq!(fs::read(original).unwrap(), bytes);
    assert_eq!(fs::read(library.join(extra)).unwrap(), bytes);
}

#[test]
fn adopts_unrecorded_media_and_repeat_changes_only_the_snapshot() {
    let (_temp, source, library) = setup();
    let extra = "_unsorted/new.PNG";
    fs::write(library.join(extra), b"new media").unwrap();
    fs::write(source.join("new.PNG"), b"new media").unwrap();
    let report = reindex(&library).unwrap();
    assert_eq!(report.adopted, vec![extra]);
    assert!(report.errors.is_empty());
    assert!(report.duplicates.is_empty());
    assert_eq!(fs::read(library.join(extra)).unwrap(), b"new media");
    assert!(check::check(&source, &library).unwrap().is_clean());

    let before = catalog(&open(&library));
    let db_bytes = fs::read(library.join("library.db")).unwrap();
    let backup_count = snapshots(&library).len();
    let report = reindex(&library).unwrap();
    assert!(report.adopted.is_empty());
    assert!(report.duplicates.is_empty());
    assert!(report.errors.is_empty());
    assert!(report.restored_from.is_none());
    assert_eq!(catalog(&open(&library)), before);
    assert_eq!(fs::read(library.join("library.db")).unwrap(), db_bytes);
    assert_eq!(snapshots(&library).len(), backup_count + 1);
}

#[test]
fn skips_corrupt_newest_snapshot() {
    let (_temp, _source, library) = setup();
    let good = fs::canonicalize(snapshots(&library).pop().unwrap()).unwrap();
    let before = catalog(&open(&library));
    let corrupt = library.join(".catalog-backups/library-99991231T235959.db");
    fs::write(&corrupt, b"not a database").unwrap();
    fs::remove_file(library.join("library.db")).unwrap();

    let report = reindex(&library).unwrap();
    assert_eq!(report.restored_from, Some(good));
    assert!(report.errors.is_empty());
    assert_eq!(catalog(&open(&library)), before);
    assert_eq!(fs::read(corrupt).unwrap(), b"not a database");
}

#[test]
fn unusable_snapshots_fall_back_to_rebuilding() {
    let (_temp, source, library) = setup();
    for snapshot in snapshots(&library) {
        fs::write(snapshot, b"not a database").unwrap();
    }
    fs::remove_file(library.join("library.db")).unwrap();
    let report = reindex(&library).unwrap();
    assert!(report.restored_from.is_none());
    assert_eq!(report.adopted.len(), 5);
    assert!(report.errors.is_empty());
    assert!(check::check(&source, &library).unwrap().is_clean());
}

#[test]
fn attaches_new_mov_and_sidecar_to_a_recorded_photo() {
    let (_temp, source, library) = setup();
    let db = open(&library);
    let photo_hash = hash::hash_file(&source.join("standalone.PNG")).unwrap();
    let photo_path = db.asset_path(&photo_hash).unwrap().unwrap();
    drop(db);
    let mov = library.join(&photo_path).with_extension("mov");
    let sidecar = library.join(&photo_path).with_extension("aae");
    fs::write(&mov, b"new live video").unwrap();
    fs::write(&sidecar, b"new attached sidecar").unwrap();
    let mov_hash = hash::hash_file(&mov).unwrap();
    let sidecar_hash = hash::hash_file(&sidecar).unwrap();

    let report = reindex(&library).unwrap();
    assert_eq!(report.adopted.len(), 2);
    assert!(report.errors.is_empty());
    let db = open(&library);
    let linked_mov: String = db
        .conn
        .query_row(
            "SELECT mov_hash FROM live_pairs WHERE photo_hash=?1",
            [&photo_hash],
            |r| r.get(0),
        )
        .unwrap();
    let linked_photo: String = db
        .conn
        .query_row(
            "SELECT asset_hash FROM sidecars WHERE sidecar_hash=?1",
            [&sidecar_hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked_mov, mov_hash);
    assert_eq!(linked_photo, photo_hash);
    assert!(!db.has_asset(&sidecar_hash).unwrap());
    assert!(check::check(&source, &library).unwrap().is_clean());
}

#[test]
fn insert_failure_rolls_back_all_adoptions() {
    let (_temp, _source, library) = setup();
    let db = Db::open(&library.join("library.db")).unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_second BEFORE INSERT ON assets WHEN NEW.library_path='_unsorted/b.png' BEGIN SELECT RAISE(FAIL, 'reject'); END;").unwrap();
    let before = catalog(&db);
    drop(db);
    fs::write(library.join("_unsorted/a.png"), b"first new media").unwrap();
    fs::write(library.join("_unsorted/b.png"), b"second new media").unwrap();

    assert!(reindex(&library).is_err());
    assert_eq!(catalog(&open(&library)), before);
    assert_eq!(
        fs::read(library.join("_unsorted/a.png")).unwrap(),
        b"first new media"
    );
    assert_eq!(
        fs::read(library.join("_unsorted/b.png")).unwrap(),
        b"second new media"
    );
}

#[test]
fn link_failure_rolls_back_assets_and_sidecars() {
    let (_temp, _source, library) = setup();
    let db = Db::open(&library.join("library.db")).unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_pair BEFORE INSERT ON live_pairs BEGIN SELECT RAISE(FAIL, 'reject'); END;").unwrap();
    let before = catalog(&db);
    drop(db);
    for (name, bytes) in [
        ("new.jpg", b"photo".as_slice()),
        ("new.mov", b"mov"),
        ("new.aae", b"aae"),
    ] {
        fs::write(library.join("_unsorted").join(name), bytes).unwrap();
    }

    assert!(reindex(&library).is_err());
    assert_eq!(catalog(&open(&library)), before);
    for name in ["new.jpg", "new.mov", "new.aae"] {
        assert!(library.join("_unsorted").join(name).is_file());
    }
}

#[test]
fn existing_damaged_catalog_is_never_replaced() {
    let (_temp, _source, library) = setup();
    let backups = snapshots(&library);
    fs::write(library.join("library.db"), b"damaged catalog").unwrap();
    assert!(reindex(&library).is_err());
    assert_eq!(
        fs::read(library.join("library.db")).unwrap(),
        b"damaged catalog"
    );
    assert_eq!(snapshots(&library), backups);
}

#[test]
fn another_library_lock_prevents_changes() {
    let (_temp, _source, library) = setup();
    let lock = fs::OpenOptions::new()
        .write(true)
        .open(library.join(".photoxfer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let before = catalog(&open(&library));
    let backups = snapshots(&library);
    assert!(reindex(&library).is_err());
    assert_eq!(catalog(&open(&library)), before);
    assert_eq!(snapshots(&library), backups);
}
