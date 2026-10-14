#[allow(dead_code)]
mod common;

use photoxfer_core::{check, dates, db::Db, hash, import::import_folder};
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

fn setup(count: usize) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let inbox = temp.path().join("inbox");
    let library = temp.path().join("library");
    std::fs::create_dir(&inbox).unwrap();
    for index in 0..count {
        let mut bytes = common::jpeg_without_exif();
        bytes.push(index as u8);
        std::fs::write(inbox.join(format!("image{index}.jpg")), bytes).unwrap();
    }
    assert_eq!(
        import_folder(&inbox, &library).unwrap().copied,
        count as u64
    );
    (temp, inbox, library)
}

fn preview(library: &Path, out: &Path, uses: &[&str]) -> Vec<String> {
    let rows = dates::undated(library).unwrap();
    dates::write_preview(&rows, out).unwrap();
    let mut lines: Vec<String> = std::fs::read_to_string(out)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    for (line, value) in lines.iter_mut().skip(1).zip(uses) {
        let mut fields: Vec<String> = line.split('\t').map(str::to_owned).collect();
        fields[4] = "2021-02-03 04:05:06".into();
        fields[5] = (*value).into();
        *line = fields.join("\t");
    }
    std::fs::write(out, format!("{}\n", lines.join("\n"))).unwrap();
    rows.into_iter().map(|r| r.library_path).collect()
}

fn assets(library: &Path) -> Vec<Vec<rusqlite::types::Value>> {
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let mut stmt = db
        .conn
        .prepare("SELECT * FROM assets ORDER BY hash")
        .unwrap();
    let count = stmt.column_count();
    stmt.query_map([], |r| (0..count).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn media(library: &Path) -> Vec<(PathBuf, String)> {
    files(library)
        .into_iter()
        .filter(|p| {
            !p.strip_prefix(library)
                .unwrap()
                .components()
                .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
                && p.file_name().unwrap() != "library.db"
        })
        .map(|p| {
            let hash = hash::hash_file(&p).unwrap();
            (p.strip_prefix(library).unwrap().to_owned(), hash)
        })
        .collect()
}

#[test]
fn apply_moves_only_accepted_dates_and_preserves_bytes() {
    let (temp, inbox, library) = setup(3);
    let tsv = temp.path().join("dates.tsv");
    let paths = preview(&library, &tsv, &["2021-02-03 04:05:06", "2022-03-04", ""]);
    let hashes: Vec<_> = paths
        .iter()
        .map(|p| hash::hash_file(&library.join(p)).unwrap())
        .collect();
    let snapshots_before = files(&library.join(".catalog-backups")).len();
    let report = dates::apply(&library, &tsv).unwrap();
    assert_eq!(report.kept_undated, 1);
    assert_eq!(report.moved.len(), 2);
    assert_eq!(
        files(&library.join(".catalog-backups")).len(),
        snapshots_before + 1
    );
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    for (index, (old, new)) in report.moved.iter().enumerate() {
        let (folder, stem_date, taken, basis) = if index == 0 {
            (
                "2021/February",
                "2021-02-03",
                "2021-02-03T04:05:06",
                "estimate",
            )
        } else {
            ("2022/March", "2022-03-04", "2022-03-04T00:00:00", "manual")
        };
        assert_eq!(old, &paths[index]);
        assert_eq!(
            new,
            &format!(
                "{folder}/{stem_date}_estimated_{}.jpg",
                hash::short_hash(&hashes[index])
            )
        );
        assert!(!library.join(old).exists());
        assert_eq!(hash::hash_file(&library.join(new)).unwrap(), hashes[index]);
        let stored: (String, String) = db
            .conn
            .query_row(
                "SELECT taken_at,date_basis FROM assets WHERE hash=?1",
                [&hashes[index]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (taken.into(), basis.into()));
    }
    assert!(paths[2].starts_with("_unsorted/"));
    assert_eq!(
        hash::hash_file(&library.join(&paths[2])).unwrap(),
        hashes[2]
    );
    assert!(check::check(&inbox, &library).unwrap().is_clean());
}

#[test]
fn invalid_preview_changes_nothing() {
    for invalid in [
        "2021-02-30",
        "2021-2-03",
        "2021-02-03T04:05:06",
        "2021-02-03 24:00:00",
        " ",
    ] {
        let (temp, _, library) = setup(2);
        let tsv = temp.path().join("dates.tsv");
        preview(&library, &tsv, &["2021-02-03", invalid]);
        let before = (assets(&library), media(&library), files(&library));
        let error = dates::apply(&library, &tsv).unwrap_err();
        assert!(format!("{error:#}").contains("line 3"));
        assert_eq!(before, (assets(&library), media(&library), files(&library)));
    }
}

#[test]
fn malformed_header_or_row_changes_nothing() {
    for malformed in [
        "wrong\n",
        "library_path\toriginal_names\tsource_created\tsource_modified\tsuggested\tuse\nonly\tfive\tfields\tremain\there\n",
    ] {
        let (temp, _, library) = setup(1);
        let tsv = temp.path().join("dates.tsv");
        std::fs::write(&tsv, malformed).unwrap();
        let before = (assets(&library), files(&library));
        assert!(dates::apply(&library, &tsv).is_err());
        assert_eq!(before, (assets(&library), files(&library)));
    }
}

#[test]
fn damaged_asset_prevents_all_moves() {
    let (temp, _, library) = setup(2);
    let tsv = temp.path().join("dates.tsv");
    let paths = preview(&library, &tsv, &["2021-02-03", "2021-02-03"]);
    std::fs::write(library.join(&paths[1]), b"damaged").unwrap();
    let before = (assets(&library), media(&library));
    assert!(
        format!("{:#}", dates::apply(&library, &tsv).unwrap_err()).contains("changed since import")
    );
    assert_eq!(before, (assets(&library), media(&library)));
}

#[test]
fn dated_unknown_unsafe_duplicate_and_live_paths_prevent_all_moves() {
    for invalid in [
        "dated",
        "unknown",
        "../outside.jpg",
        "_unsorted//file.jpg",
        "/absolute.jpg",
        "duplicate",
        "live-photo",
        "live-mov",
        "sidecar",
    ] {
        let (temp, _, library) = setup(2);
        let tsv = temp.path().join("dates.tsv");
        let paths = preview(&library, &tsv, &["2021-02-03", "2021-02-03"]);
        let db = Db::open(&library.join("library.db")).unwrap();
        let replacement = match invalid {
            "dated" => {
                db.conn.execute("UPDATE assets SET taken_at='2020-01-01T00:00:00',date_basis='metadata' WHERE library_path=?1", [&paths[1]]).unwrap();
                paths[1].clone()
            }
            "sidecar" => {
                db.conn
                    .execute(
                        "UPDATE assets SET kind='sidecar' WHERE library_path=?1",
                        [&paths[1]],
                    )
                    .unwrap();
                paths[1].clone()
            }
            "live-photo" | "live-mov" => {
                let a = hash::hash_file(&library.join(&paths[0])).unwrap();
                let b = hash::hash_file(&library.join(&paths[1])).unwrap();
                db.link_live_pair(&a, &b).unwrap();
                if invalid == "live-photo" {
                    paths[0].clone()
                } else {
                    paths[1].clone()
                }
            }
            "duplicate" => paths[0].clone(),
            "unknown" => "_unsorted/unknown.jpg".into(),
            path => path.into(),
        };
        drop(db);
        let text = std::fs::read_to_string(&tsv).unwrap();
        let mut lines: Vec<_> = text.lines().map(str::to_owned).collect();
        let mut fields: Vec<_> = lines[2].split('\t').map(str::to_owned).collect();
        fields[0] = replacement;
        lines[2] = fields.join("\t");
        std::fs::write(&tsv, lines.join("\n")).unwrap();
        let before = (assets(&library), media(&library));
        assert!(dates::apply(&library, &tsv).is_err(), "{invalid}");
        assert_eq!(before, (assets(&library), media(&library)), "{invalid}");
    }
}

#[test]
fn apply_moves_linked_aae_and_check_stays_clean() {
    let (temp, inbox, library) = setup(1);
    std::fs::write(inbox.join("image0.AAE"), b"edit metadata").unwrap();
    assert_eq!(import_folder(&inbox, &library).unwrap().copied, 1);
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let (side_hash, old_side): (String, String) = db
        .conn
        .query_row("SELECT sidecar_hash,library_path FROM sidecars", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    drop(db);
    let tsv = temp.path().join("dates.tsv");
    preview(&library, &tsv, &["2021-02-03"]);
    let report = dates::apply(&library, &tsv).unwrap();
    assert_eq!(report.moved.len(), 1);
    let db = Db::open_read_only(&library.join("library.db")).unwrap();
    let new_side: String = db
        .conn
        .query_row("SELECT library_path FROM sidecars", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        Path::new(&new_side),
        Path::new(&report.moved[0].1).with_extension("aae")
    );
    assert!(!library.join(&old_side).exists());
    assert_eq!(
        hash::hash_file(&library.join(&new_side)).unwrap(),
        side_hash
    );
    assert!(check::check(&inbox, &library).unwrap().is_clean());
}

#[test]
fn second_apply_fails_without_changes() {
    let (temp, _, library) = setup(2);
    let tsv = temp.path().join("dates.tsv");
    preview(&library, &tsv, &["2021-02-03", "2021-02-03"]);
    dates::apply(&library, &tsv).unwrap();
    let before = (assets(&library), media(&library));
    assert!(dates::apply(&library, &tsv).is_err());
    assert_eq!(before, (assets(&library), media(&library)));
}

#[test]
fn transaction_failure_removes_links_and_reports_completed_rows() {
    let (temp, inbox, library) = setup(2);
    let tsv = temp.path().join("dates.tsv");
    let paths = preview(&library, &tsv, &["2021-02-03", "2021-02-03"]);
    let db = Db::open(&library.join("library.db")).unwrap();
    db.conn.execute_batch(&format!(
        "CREATE TRIGGER reject_date BEFORE UPDATE ON assets WHEN OLD.library_path='{}' BEGIN SELECT RAISE(ABORT,'test failure'); END;", paths[1]
    )).unwrap();
    drop(db);
    let error = format!("{:#}", dates::apply(&library, &tsv).unwrap_err());
    assert!(error.contains("test failure"));
    assert!(error.contains("already moved"));
    assert!(error.contains(&paths[0]));
    assert!(!library.join(&paths[0]).exists());
    assert!(library.join(&paths[1]).exists());
    assert_eq!(dates::undated(&library).unwrap().len(), 1);
    assert_eq!(media(&library).len(), 2);
    assert!(check::check(&inbox, &library).unwrap().is_clean());
}

#[test]
fn sidecar_damage_link_collision_and_transaction_failure_preserve_old_files() {
    for failure in ["damage", "collision", "transaction"] {
        let (temp, inbox, library) = setup(1);
        std::fs::write(inbox.join("image0.AAE"), b"edit metadata").unwrap();
        import_folder(&inbox, &library).unwrap();
        let tsv = temp.path().join("dates.tsv");
        let paths = preview(&library, &tsv, &["2021-02-03"]);
        let db = Db::open(&library.join("library.db")).unwrap();
        let old_side: String = db
            .conn
            .query_row("SELECT library_path FROM sidecars", [], |r| r.get(0))
            .unwrap();
        match failure {
            "damage" => std::fs::write(library.join(&old_side), b"damaged sidecar").unwrap(),
            "collision" => {
                let asset_hash = hash::hash_file(&library.join(&paths[0])).unwrap();
                let folder = library.join("2021/February");
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(
                    folder.join(format!("2021-02-03_estimated_{}.aae", hash::short_hash(&asset_hash))),
                    b"preserve this conflicting file",
                ).unwrap();
            }
            "transaction" => db.conn.execute_batch(
                "CREATE TRIGGER reject_sidecar BEFORE UPDATE ON sidecars BEGIN SELECT RAISE(ABORT,'sidecar failure'); END;",
            ).unwrap(),
            _ => unreachable!(),
        }
        let sidecars_before = db.recorded_files().unwrap();
        drop(db);
        let before = (assets(&library), media(&library));
        assert!(dates::apply(&library, &tsv).is_err(), "{failure}");
        assert_eq!(before, (assets(&library), media(&library)), "{failure}");
        let db = Db::open_read_only(&library.join("library.db")).unwrap();
        assert_eq!(sidecars_before, db.recorded_files().unwrap());
        if failure == "transaction" {
            assert!(check::check(&inbox, &library).unwrap().is_clean());
        }
    }
}
