#[allow(dead_code)]
mod common;
use photoxfer_core::{
    db::Db,
    hash,
    import::{copy_verified, import_folder},
    layout, metadata,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    (temp, inbox, library)
}
fn files(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(files(&path));
        } else {
            result.push(path);
        }
    }
    result.sort();
    result
}
fn target(src: &Path, library: &Path) -> PathBuf {
    let taken = metadata::capture_time(src);
    library.join(layout::dest_dir(taken)).join(format!(
        "{}.jpg",
        layout::file_stem(taken, &hash::hash_file(src).unwrap())
    ))
}
#[test]
fn dated_and_undated_layout_repeat_and_finished_counts() {
    let (_temp, inbox, library) = setup();
    let dated = inbox.join("dated.JPG");
    fs::write(&dated, common::jpeg_with_date()).unwrap();
    let undated = inbox.join("undated.jpg");
    fs::write(&undated, common::jpeg_without_exif()).unwrap();
    let report = import_folder(&inbox, &library).unwrap();
    assert_eq!((report.found, report.copied, report.failed), (2, 2, 0));
    let path = target(&dated, &library);
    assert!(path.starts_with(library.join("2026/10")));
    assert_eq!(fs::read(path).unwrap(), fs::read(&dated).unwrap());
    assert!(target(&undated, &library).starts_with(library.join("_unsorted")));
    assert!(target(&undated, &library).is_file());
    let before = files(&library);
    let report = import_folder(&inbox, &library).unwrap();
    assert_eq!((report.copied, report.skipped, report.found), (0, 2, 2));
    assert_eq!(before, files(&library));
    let db = Db::open(&library.join("library.db")).unwrap();
    let rows: Vec<(Option<String>, i64, i64, i64)> = db
        .conn
        .prepare("SELECT finished_at,copied,skipped,failed FROM imports ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.0.is_some()));
    assert_eq!((rows[0].1, rows[0].2, rows[0].3), (2, 0, 0));
    assert_eq!((rows[1].1, rows[1].2, rows[1].3), (0, 2, 0));
}
#[test]
fn identical_bytes_are_deduplicated() {
    let (_temp, inbox, library) = setup();
    for name in ["a.jpg", "b.jpg"] {
        fs::write(inbox.join(name), common::jpeg_with_date()).unwrap();
    }
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.found, r.copied, r.skipped), (2, 1, 1));
}
#[test]
fn existing_collision_gets_suffix_and_is_untouched() {
    let (_temp, inbox, library) = setup();
    let src = inbox.join("a.jpg");
    fs::write(&src, common::jpeg_with_date()).unwrap();
    let mut second = common::jpeg_with_date();
    second.push(42);
    fs::write(inbox.join("b.jpg"), second).unwrap();
    let original = target(&src, &library);
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    fs::write(&original, b"original different bytes").unwrap();
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!(r.copied, 2);
    assert_eq!(fs::read(&original).unwrap(), b"original different bytes");
    let suffix = original.with_file_name(format!(
        "{}-1.jpg",
        original.file_stem().unwrap().to_string_lossy()
    ));
    assert_eq!(fs::read(suffix).unwrap(), fs::read(src).unwrap());
}
#[test]
fn live_pair_and_sidecar_share_primary_stem() {
    let (_temp, inbox, library) = setup();
    let primary = inbox.join("IMG_1.HEIC");
    fs::write(&primary, common::jpeg_with_date()).unwrap();
    fs::write(inbox.join("IMG_1.MOV"), common::mov_with_creation_time(0)).unwrap();
    fs::write(inbox.join("IMG_1.AAE"), b"sidecar").unwrap();
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.copied, r.failed), (3, 0));
    let base = target(&primary, &library);
    for ext in ["heic", "mov", "aae"] {
        assert!(base.with_extension(ext).is_file());
    }
    let db = Db::open(&library.join("library.db")).unwrap();
    for table in ["live_pairs", "sidecars"] {
        let count: i64 = db
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
    let kind: String = db
        .conn
        .query_row(
            "SELECT kind FROM assets WHERE hash=?1",
            [hash::hash_file(&primary).unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "live");
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.copied, r.skipped, r.found), (0, 3, 3));
}
#[test]
fn copy_hash_mismatch_cleans_temporary_file() {
    let (_temp, inbox, library) = setup();
    let src = inbox.join("source");
    fs::write(&src, b"bytes").unwrap();
    let dest = library.join("dest");
    let error = copy_verified(&src, &dest, "wrong").unwrap_err().to_string();
    assert!(error.contains("wrong") && error.contains(&hash::hash_file(&src).unwrap()));
    assert!(!dest.exists());
    assert!(files(&library).is_empty());
}
#[test]
fn copy_existing_destination_is_untouched_and_cleans_temp() {
    let (_temp, inbox, library) = setup();
    fs::create_dir(&library).unwrap();
    let src = inbox.join("source");
    let dest = library.join("dest");
    fs::write(&src, b"new").unwrap();
    fs::write(&dest, b"old").unwrap();
    assert!(copy_verified(&src, &dest, &hash::hash_file(&src).unwrap()).is_err());
    assert_eq!(fs::read(&dest).unwrap(), b"old");
    assert_eq!(files(&library), vec![dest]);
}
#[test]
fn nested_library_and_hidden_entries_are_excluded() {
    let (_temp, inbox, _) = setup();
    let library = inbox.join("library");
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    fs::write(inbox.join(".hidden"), b"hidden").unwrap();
    fs::create_dir(inbox.join(".hidden-dir")).unwrap();
    fs::write(inbox.join(".hidden-dir/file"), b"hidden").unwrap();
    assert_eq!(import_folder(&inbox, &library).unwrap().found, 1);
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.found, r.skipped), (1, 1));
}
#[test]
fn db_insert_failure_removes_published_copy() {
    let (_temp, inbox, library) = setup();
    fs::create_dir(&library).unwrap();
    let db = Db::open(&library.join("library.db")).unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_asset BEFORE INSERT ON assets BEGIN SELECT RAISE(FAIL, 'reject'); END;").unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    assert!(import_folder(&inbox, &library).is_err());
    assert!(
        files(&library)
            .iter()
            .all(|p| p.file_name().unwrap() == "library.db")
    );
}
#[test]
fn orphan_sidecar_is_imported_as_its_own_asset() {
    let (_temp, inbox, library) = setup();
    let src = inbox.join("orphan.AAE");
    fs::write(&src, b"sidecar").unwrap();
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.found, r.failed, r.copied), (1, 0, 1));
    let file_hash = hash::hash_file(&src).unwrap();
    let dest = library
        .join("_unsorted")
        .join(format!("{}.aae", layout::file_stem(None, &file_hash)));
    assert_eq!(fs::read(dest).unwrap(), b"sidecar");
    let db = Db::open(&library.join("library.db")).unwrap();
    let kind: String = db
        .conn
        .query_row("SELECT kind FROM assets WHERE hash=?1", [&file_hash], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kind, "sidecar");
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.copied, r.skipped), (0, 1));
}

#[test]
fn missing_source_cleans_temporary_file() {
    let (_temp, inbox, library) = setup();
    assert!(copy_verified(&inbox.join("missing"), &library.join("dest"), "hash").is_err());
    assert!(files(&library).is_empty());
}

#[test]
fn sidecar_db_failure_removes_only_sidecar_copy() {
    let (_temp, inbox, library) = setup();
    fs::create_dir(&library).unwrap();
    let db = Db::open(&library.join("library.db")).unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_sidecar BEFORE INSERT ON sidecars BEGIN SELECT RAISE(FAIL, 'reject'); END;").unwrap();
    let src = inbox.join("a.jpg");
    fs::write(&src, common::jpeg_with_date()).unwrap();
    fs::write(inbox.join("a.aae"), b"sidecar").unwrap();
    assert!(import_folder(&inbox, &library).is_err());
    let base = target(&src, &library);
    assert!(base.is_file());
    assert!(!base.with_extension("aae").exists());
    assert!(db.has_asset(&hash::hash_file(&src).unwrap()).unwrap());
}

#[test]
fn recursive_inbox_and_orphan_mov_are_imported() {
    let (_temp, inbox, library) = setup();
    fs::create_dir(inbox.join("nested")).unwrap();
    let src = inbox.join("nested/orphan.MOV");
    fs::write(
        &src,
        common::mov_with_offset_datetime("2026-10-07T14:30:12-07:00"),
    )
    .unwrap();
    let r = import_folder(&inbox, &library).unwrap();
    assert_eq!((r.found, r.copied, r.failed), (1, 1, 0));
    let taken = metadata::capture_time(&src);
    let dest = library.join(layout::dest_dir(taken)).join(format!(
        "{}.mov",
        layout::file_stem(taken, &hash::hash_file(&src).unwrap())
    ));
    assert_eq!(fs::read(dest).unwrap(), fs::read(&src).unwrap());
}
