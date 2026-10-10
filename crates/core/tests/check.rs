#[allow(dead_code)]
mod common;
use photoxfer_core::{check::check, db::Db, import::import_folder};
use std::{fs, path::PathBuf};

fn imported() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_with_date()).unwrap();
    fs::write(inbox.join("b.jpg"), common::jpeg_without_exif()).unwrap();
    fs::write(inbox.join("desktop.ini"), b"[.ShellClassInfo]").unwrap();
    assert_eq!(import_folder(&inbox, &library).unwrap().copied, 2);
    (temp, inbox, library)
}

fn library_path(library: &std::path::Path, name: &str) -> PathBuf {
    let db = Db::open(&library.join("library.db")).unwrap();
    let hash =
        photoxfer_core::hash::hash_file(&library.join("..").join("inbox").join(name)).unwrap();
    library.join(db.asset_path(&hash).unwrap().unwrap())
}

#[test]
fn imported_files_are_all_safe() {
    let (_temp, inbox, library) = imported();
    let r = check(&inbox, &library).unwrap();
    assert_eq!(r.safe.len(), 2);
    assert!(r.missing.is_empty() && r.library_problems.is_empty() && r.untracked.is_empty());
    assert!(r.errors.is_empty());
    assert_eq!(r.ignored, 1);
}

#[test]
fn new_source_file_is_reported_missing() {
    let (_temp, inbox, library) = imported();
    fs::write(
        inbox.join("new.jpg"),
        common::jpeg_with_datetime("2024:01:02 03:04:05"),
    )
    .unwrap();
    let r = check(&inbox, &library).unwrap();
    assert_eq!(r.safe.len(), 2);
    assert_eq!(r.missing.len(), 1);
    assert!(r.missing[0].ends_with("new.jpg"));
}

#[test]
fn damaged_library_copy_is_not_safe() {
    let (_temp, inbox, library) = imported();
    fs::write(library_path(&library, "a.jpg"), b"corrupted").unwrap();
    let r = check(&inbox, &library).unwrap();
    assert_eq!(r.library_problems.len(), 1);
    assert!(r.library_problems[0].contains("contents changed"));
    assert_eq!(r.missing.len(), 1);
    assert!(r.missing[0].ends_with("a.jpg"));
}

#[test]
fn deleted_library_copy_is_not_safe() {
    let (_temp, inbox, library) = imported();
    fs::remove_file(library_path(&library, "b.jpg")).unwrap();
    let r = check(&inbox, &library).unwrap();
    assert_eq!(
        (r.safe.len(), r.missing.len(), r.library_problems.len()),
        (1, 1, 1)
    );
}

#[test]
fn stray_library_file_is_untracked() {
    let (_temp, inbox, library) = imported();
    fs::write(library.join("_unsorted").join("stray.jpg"), b"stray").unwrap();
    let r = check(&inbox, &library).unwrap();
    assert_eq!(r.untracked.len(), 1);
    assert!(r.untracked[0].ends_with("stray.jpg"));
}

#[test]
fn check_never_creates_a_library() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir_all(&inbox).unwrap();
    fs::create_dir_all(&library).unwrap();
    assert!(check(&inbox, &library).is_err());
    assert!(!library.join("library.db").exists());
}
