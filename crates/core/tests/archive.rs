#[allow(dead_code)]
mod common;
use photoxfer_core::{
    archive::{archive, verify},
    db::Db,
    import::import_folder,
};
use std::fs;

#[test]
fn archive_is_independent_repeatable_and_restorable() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let destination = temp.path().join("archive");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("IMG_3353.PNG"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let matches = db.find("img_3353").unwrap();
    assert_eq!(matches.len(), 1);
    let relative = &matches[0].1;
    let first = archive(&library, &destination).unwrap();
    assert_eq!((first.copied, first.verified), (1, 1));
    assert_eq!(verify(&destination, &first.manifest).unwrap(), 2);
    assert_eq!(archive(&library, &destination).unwrap().copied, 0);
    // A change to the working copy must not affect the archive (no hard link to source).
    fs::write(library.join(relative), b"changed working copy").unwrap();
    assert_eq!(verify(&destination, &first.manifest).unwrap(), 2);
    assert!(archive(&library, &destination).is_err());
    fs::write(destination.join(relative), b"damaged archive").unwrap();
    assert!(verify(&destination, &first.manifest).is_err());
}

#[test]
fn archive_preserves_conflicts_and_missing_source_items() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let destination = temp.path().join("archive");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let r = archive(&library, &destination).unwrap();
    fs::remove_file(inbox.join("a.jpg")).unwrap();
    import_folder(&inbox, &library).unwrap();
    assert_eq!(archive(&library, &destination).unwrap().verified, 1);
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let path = destination.join(&db.recorded_files().unwrap()[0].1);
    fs::write(&path, b"existing conflict").unwrap();
    assert!(archive(&library, &destination).is_err());
    assert_eq!(fs::read(path).unwrap(), b"existing conflict");
    assert!(verify(&destination, &r.manifest).is_err());
}

#[test]
fn manifest_rejects_paths_outside_restore_folder() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = temp.path().join("manifest.tsv");
    fs::write(&manifest, "photoxfer-blake3-v1\nabc\t3\t../outside.jpg\n").unwrap();
    assert!(verify(temp.path(), &manifest).is_err());
}

#[test]
fn database_snapshot_recovers_source_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("first.jpg"), common::jpeg_without_exif()).unwrap();
    fs::write(inbox.join("second.jpg"), common::jpeg_without_exif()).unwrap();
    let report = import_folder(&inbox, &library).unwrap();
    assert_eq!((report.copied, report.skipped), (1, 1));
    let snapshot = fs::read_dir(library.join(".catalog-backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let recovered = Db::open_read_only(&snapshot).unwrap();
    assert_eq!(recovered.find(".jpg").unwrap().len(), 2);
    let integrity: String = recovered
        .conn
        .query_row("pragma integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn truncated_manifest_cannot_report_a_complete_restore() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let destination = temp.path().join("archive");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("a.jpg"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let report = archive(&library, &destination).unwrap();
    let text = fs::read_to_string(&report.manifest).unwrap();
    let mut lines: Vec<_> = text.lines().collect();
    lines.pop();
    fs::write(&report.manifest, lines.join("\n")).unwrap();
    assert!(verify(&destination, &report.manifest).is_err());
}
