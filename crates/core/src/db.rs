use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub struct Db {
    pub conn: Connection,
}

pub struct AssetRow {
    pub hash: String,
    pub kind: String,
    pub taken_at: Option<String>,
    pub library_path: String,
}

impl Db {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db { conn };
        let old_catalog: bool = db.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='assets') AND NOT EXISTS(SELECT 1 FROM sqlite_master WHERE name='source_files')",
            [], |r| r.get(0),
        )?;
        if old_catalog {
            db.snapshot(
                &path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(".catalog-backups"),
            )?;
        }
        db.migrate()?;
        Ok(db)
    }

    pub fn open_read_only(path: &Path) -> rusqlite::Result<Db> {
        Ok(Db {
            conn: Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        })
    }

    pub fn snapshot(&self, directory: &Path) -> rusqlite::Result<PathBuf> {
        std::fs::create_dir_all(directory)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let path = directory.join(format!(
            "library-{}.db",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ")
        ));
        let reservation = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        drop(reservation);
        self.conn.backup("main", &path, None)?;
        Ok(path)
    }

    pub fn record_source(&self, hash: &str, source: &Path) -> rusqlite::Result<()> {
        let name = source.file_name().unwrap_or_default().to_string_lossy();
        self.conn.execute(
            "INSERT OR IGNORE INTO source_files(hash,original_name,source_path) VALUES(?1,?2,?3)",
            params![hash, name, source.to_string_lossy()],
        )?;
        Ok(())
    }

    pub fn recorded_files(&self) -> rusqlite::Result<Vec<(String, String)>> {
        self.conn.prepare("SELECT hash,library_path FROM assets UNION ALL SELECT sidecar_hash,library_path FROM sidecars")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    }

    pub fn stored_path(&self, hash: &str) -> rusqlite::Result<Option<String>> {
        self.conn.query_row(
            "SELECT library_path FROM assets WHERE hash=?1 UNION ALL SELECT library_path FROM sidecars WHERE sidecar_hash=?1 LIMIT 1",
            [hash], |r| r.get(0),
        ).optional()
    }

    pub fn find(&self, text: &str) -> rusqlite::Result<Vec<(String, String)>> {
        self.conn.prepare("SELECT DISTINCT s.original_name,p.library_path FROM source_files s JOIN (SELECT hash,library_path FROM assets UNION ALL SELECT sidecar_hash,library_path FROM sidecars) p ON p.hash=s.hash WHERE instr(lower(s.original_name),lower(?1))>0 OR instr(lower(s.source_path),lower(?1))>0 ORDER BY s.original_name,p.library_path")?
            .query_map([text], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    }

    pub fn open_in_memory() -> rusqlite::Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Create tables assets, device_seen, live_pairs, sidecars, imports if missing.
    pub fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS assets(hash TEXT PRIMARY KEY, kind TEXT NOT NULL, taken_at TEXT, width INTEGER, height INTEGER, gps_lat REAL, gps_lon REAL, camera TEXT, library_path TEXT NOT NULL UNIQUE, imported_at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS device_seen(device_id TEXT NOT NULL, object_id TEXT NOT NULL, size INTEGER NOT NULL, hash TEXT NOT NULL, PRIMARY KEY(device_id, object_id, size));
            CREATE TABLE IF NOT EXISTS live_pairs(photo_hash TEXT PRIMARY KEY REFERENCES assets(hash), mov_hash TEXT NOT NULL REFERENCES assets(hash));
            CREATE TABLE IF NOT EXISTS sidecars(sidecar_hash TEXT PRIMARY KEY, asset_hash TEXT NOT NULL REFERENCES assets(hash), library_path TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS imports(id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT, copied INTEGER, skipped INTEGER, failed INTEGER, errors TEXT);
            CREATE TABLE IF NOT EXISTS source_files(hash TEXT NOT NULL, original_name TEXT NOT NULL, source_path TEXT NOT NULL, PRIMARY KEY(hash,source_path));")
    }

    pub fn has_asset(&self, hash: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM assets WHERE hash=?1)",
            [hash],
            |row| row.get(0),
        )
    }

    pub fn insert_asset(&self, a: &AssetRow) -> rusqlite::Result<()> {
        self.conn.execute("INSERT INTO assets(hash,kind,taken_at,library_path,imported_at) VALUES(?1,?2,?3,?4,?5)", params![a.hash, a.kind, a.taken_at, a.library_path, chrono::Local::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn asset_path(&self, hash: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT library_path FROM assets WHERE hash=?1",
                [hash],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn seen(
        &self,
        device_id: &str,
        object_id: &str,
        size: u64,
    ) -> rusqlite::Result<Option<String>> {
        let size = i64::try_from(size)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn
            .query_row(
                "SELECT hash FROM device_seen WHERE device_id=?1 AND object_id=?2 AND size=?3",
                params![device_id, object_id, size],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn mark_seen(
        &self,
        device_id: &str,
        object_id: &str,
        size: u64,
        hash: &str,
    ) -> rusqlite::Result<()> {
        let size = i64::try_from(size)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO device_seen(device_id,object_id,size,hash) VALUES(?1,?2,?3,?4)",
            params![device_id, object_id, size, hash],
        )?;
        Ok(())
    }

    pub fn link_live_pair(&self, photo_hash: &str, mov_hash: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO live_pairs(photo_hash,mov_hash) VALUES(?1,?2)",
            params![photo_hash, mov_hash],
        )?;
        Ok(())
    }

    pub fn link_sidecar(
        &self,
        asset_hash: &str,
        sidecar_hash: &str,
        library_path: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO sidecars(sidecar_hash,asset_hash,library_path) VALUES(?1,?2,?3)", params![sidecar_hash, asset_hash, library_path])?;
        Ok(())
    }

    /// Returns the new import id.
    pub fn begin_import(&self, source: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO imports(source,started_at) VALUES(?1,?2)",
            params![source, chrono::Local::now().to_rfc3339()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_import(
        &self,
        id: i64,
        copied: u64,
        skipped: u64,
        failed: u64,
        errors: &str,
    ) -> rusqlite::Result<()> {
        let convert =
            |v| i64::try_from(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)));
        self.conn.execute("UPDATE imports SET finished_at=?1,copied=?2,skipped=?3,failed=?4,errors=?5 WHERE id=?6", params![chrono::Local::now().to_rfc3339(), convert(copied)?, convert(skipped)?, convert(failed)?, errors, id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(hash: &str, path: &str) -> AssetRow {
        AssetRow {
            hash: hash.into(),
            kind: "photo".into(),
            taken_at: None,
            library_path: path.into(),
        }
    }
    #[test]
    fn migrate_twice_is_idempotent() {
        let db = Db::open_in_memory().expect("db");
        db.migrate().expect("first");
        db.migrate().expect("second");
    }
    #[test]
    fn asset_roundtrip() {
        let db = Db::open_in_memory().expect("db");
        assert!(!db.has_asset("unknown").expect("has"));
        assert_eq!(db.asset_path("unknown").expect("path"), None);
        db.insert_asset(&asset("p", "p.jpg")).expect("insert");
        assert!(db.has_asset("p").expect("has"));
        assert_eq!(db.asset_path("p").expect("path").as_deref(), Some("p.jpg"));
        let imported: String = db
            .conn
            .query_row("SELECT imported_at FROM assets", [], |r| r.get(0))
            .expect("row");
        assert!(chrono::DateTime::parse_from_rfc3339(&imported).is_ok());
    }
    #[test]
    fn duplicate_assets_fail_and_preserve_links() {
        let db = Db::open_in_memory().expect("db");
        db.insert_asset(&asset("p", "p.jpg")).expect("photo");
        db.insert_asset(&asset("m", "m.mov")).expect("mov");
        db.link_live_pair("p", "m").expect("pair");
        db.link_sidecar("p", "s", "p.aae").expect("sidecar");
        assert!(db.insert_asset(&asset("p", "new.jpg")).is_err());
        assert!(db.insert_asset(&asset("new", "p.jpg")).is_err());
        assert_eq!(db.asset_path("p").expect("path").as_deref(), Some("p.jpg"));
        for table in ["live_pairs", "sidecars"] {
            let count: i64 = db
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .expect("count");
            assert_eq!(count, 1);
        }
    }
    #[test]
    fn seen_then_mark_seen() {
        let db = Db::open_in_memory().expect("db");
        assert_eq!(db.seen("d", "o", 12).expect("query"), None);
        db.mark_seen("d", "o", 12, "p").expect("mark");
        assert_eq!(db.seen("d", "o", 12).expect("query").as_deref(), Some("p"));
        db.mark_seen("d", "o", 12, "q").expect("replace");
        assert_eq!(db.seen("d", "o", 12).expect("query").as_deref(), Some("q"));
    }
    #[test]
    fn begin_and_finish_import() {
        let db = Db::open_in_memory().expect("db");
        let id = db.begin_import("device").expect("begin");
        let before: Option<String> = db
            .conn
            .query_row("SELECT finished_at FROM imports WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .expect("row");
        assert_eq!(before, None);
        db.finish_import(id, 1, 2, 3, "problem").expect("finish");
        let row: (String, String, String, i64, i64, i64, String) = db.conn.query_row("SELECT source,started_at,finished_at,copied,skipped,failed,errors FROM imports WHERE id=?1", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).expect("row");
        assert_eq!(row.0, "device");
        assert!(chrono::DateTime::parse_from_rfc3339(&row.1).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&row.2).is_ok());
        assert_eq!((row.3, row.4, row.5, row.6), (1, 2, 3, "problem".into()));
    }
    #[test]
    fn live_pair_unknown_hash_fails() {
        let db = Db::open_in_memory().expect("db");
        db.insert_asset(&asset("p", "p.jpg")).expect("insert");
        assert!(db.link_live_pair("missing", "p").is_err());
        assert!(db.link_live_pair("p", "missing").is_err());
    }
    #[test]
    fn sidecar_unknown_asset_fails() {
        let db = Db::open_in_memory().expect("db");
        assert!(db.link_sidecar("missing", "s", "s.aae").is_err());
    }
    #[test]
    fn oversized_u64_returns_conversion_error() {
        let db = Db::open_in_memory().expect("db");
        let size = i64::MAX as u64 + 1;
        assert!(matches!(
            db.seen("d", "o", size),
            Err(rusqlite::Error::ToSqlConversionFailure(_))
        ));
        assert!(matches!(
            db.mark_seen("d", "o", size, "p"),
            Err(rusqlite::Error::ToSqlConversionFailure(_))
        ));
        let id = db.begin_import("device").expect("begin");
        for counts in [(size, 0, 0), (0, size, 0), (0, 0, size)] {
            assert!(matches!(
                db.finish_import(id, counts.0, counts.1, counts.2, ""),
                Err(rusqlite::Error::ToSqlConversionFailure(_))
            ));
        }
    }
    #[test]
    fn open_creates_file_and_enables_foreign_keys() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("library.db");
        let db = Db::open(&path).expect("open");
        assert!(path.is_file());
        assert!(db.link_sidecar("missing", "s", "s.aae").is_err());
    }
}
