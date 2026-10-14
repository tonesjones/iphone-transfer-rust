#[allow(dead_code)]
mod common;

use photoxfer_core::{dates, db::Db, import::import_folder};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

fn files(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut result = Vec::new();
    walk(root, &mut result);
    result.sort();
    result
}

#[test]
fn migration_adds_and_backfills_date_basis() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE assets(hash TEXT PRIMARY KEY, kind TEXT NOT NULL, taken_at TEXT, width INTEGER, height INTEGER, gps_lat REAL, gps_lon REAL, camera TEXT, library_path TEXT NOT NULL UNIQUE, imported_at TEXT NOT NULL);
        INSERT INTO assets(hash,kind,taken_at,library_path,imported_at) VALUES('dated','photo','2020-01-01','a.jpg','now'),('undated','photo',NULL,'b.jpg','now');").unwrap();
    drop(conn);
    let db = Db::open(&path).unwrap();
    let dated: Option<String> = db
        .conn
        .query_row(
            "SELECT date_basis FROM assets WHERE hash='dated'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dated.as_deref(), Some("metadata"));
    db.migrate().unwrap();
    let columns: i64 = db
        .conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('assets') WHERE name='date_basis'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(columns, 1);
    assert_eq!(
        db.conn
            .query_row::<Option<String>, _, _>(
                "SELECT date_basis FROM assets WHERE hash='undated'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        None
    );
}

#[test]
fn undated_suggests_the_earlier_source_timestamp() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    std::fs::create_dir(&inbox).unwrap();
    std::fs::write(inbox.join("IMG_0670.jpeg"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let rows = dates::undated(&library).unwrap();
    assert_eq!(rows.len(), 1);
    let (created, modified) = (rows[0].created.unwrap(), rows[0].modified.unwrap());
    assert_eq!(rows[0].suggested, Some(created.min(modified)));
}

#[test]
fn undated_lists_assets_and_missing_sources_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    std::fs::create_dir(&inbox).unwrap();
    let source = inbox.join("image.png");
    std::fs::write(&source, common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let db = Db::open(&library.join("library.db")).unwrap();
    let hash: String = db
        .conn
        .query_row("SELECT hash FROM assets WHERE taken_at IS NULL", [], |r| {
            r.get(0)
        })
        .unwrap();
    db.record_source(&hash, &inbox.join("gone.png")).unwrap();
    db.insert_asset(&photoxfer_core::db::AssetRow {
        hash: "side".into(),
        kind: "sidecar".into(),
        taken_at: None,
        library_path: "side.aae".into(),
    })
    .unwrap();
    drop(db);
    std::fs::remove_file(source).unwrap();
    let before_files = files(&library);
    let db_hash = blake3::hash(&std::fs::read(library.join("library.db")).unwrap());
    let rows = dates::undated(&library).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].original_names, "gone.png, image.png");
    assert!(rows[0].created.is_none() && rows[0].modified.is_none() && rows[0].suggested.is_none());
    let out = temp.path().join("preview.tsv");
    dates::write_preview(&rows, &out).unwrap();
    assert!(dates::write_preview(&rows, &out).is_err());
    assert_eq!(before_files, files(&library));
    assert_eq!(
        db_hash,
        blake3::hash(&std::fs::read(library.join("library.db")).unwrap())
    );
    let tsv = std::fs::read_to_string(out).unwrap();
    assert!(tsv.starts_with(
        "library_path\toriginal_names\tsource_created\tsource_modified\tsuggested\tuse\n"
    ));
}
