#[allow(dead_code)]
mod common;
use photoxfer_core::{
    archive::{archive, verify},
    check::check,
    db::Db,
    import::import_folder,
};
use std::fs;

#[test]
fn renamed_library_file_is_followed_by_catalog_and_archive() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    let destination = temp.path().join("archive");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("IMG_1.PNG"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    archive(&library, &destination).unwrap();

    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let old = db.find("img_1").unwrap()[0].1.clone();
    drop(db);
    let new = "Sofi Receipt.png";
    let new_path = library.join(&old).with_file_name(new);
    fs::rename(library.join(&old), &new_path).unwrap();
    let bytes = fs::read(&new_path).unwrap();

    let report = check(&inbox, &library).unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(report.renamed.len(), 1);

    let second = import_folder(&inbox, &library).unwrap();
    assert_eq!(second.copied, 0);
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    assert_eq!(db.find("sofi").unwrap().len(), 1);
    assert_eq!(db.find("img_1").unwrap().len(), 1);
    drop(db);
    assert!(check(&inbox, &library).unwrap().renamed.is_empty());

    let report = archive(&library, &destination).unwrap();
    assert_eq!(report.copied, 0);
    assert!(destination.join(&old).with_file_name(new).is_file());
    assert!(!destination.join(&old).exists());
    verify(&destination, &report.manifest).unwrap();
    assert_eq!(fs::read(&new_path).unwrap(), bytes);
}

#[test]
fn missing_file_without_matching_copy_is_still_damage() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("IMG_1.PNG"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let old = db.find("img_1").unwrap()[0].1.clone();
    drop(db);
    fs::write(library.join(&old).with_file_name("other.png"), b"different").unwrap();
    fs::remove_file(library.join(&old)).unwrap();
    let report = check(&inbox, &library).unwrap();
    assert!(!report.is_clean());
    assert!(report.renamed.is_empty());
}

#[test]
fn copy_of_a_file_that_still_exists_stays_untracked() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    fs::create_dir(&inbox).unwrap();
    fs::write(inbox.join("IMG_1.PNG"), common::jpeg_without_exif()).unwrap();
    import_folder(&inbox, &library).unwrap();
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let old = db.find("img_1").unwrap()[0].1.clone();
    drop(db);
    fs::copy(
        library.join(&old),
        library.join(&old).with_file_name("copy.png"),
    )
    .unwrap();
    let report = check(&inbox, &library).unwrap();
    assert!(report.renamed.is_empty());
    assert_eq!(report.untracked.len(), 1);
}
