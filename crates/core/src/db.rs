use rusqlite::Connection;

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
        todo!()
    }

    pub fn open_in_memory() -> rusqlite::Result<Db> {
        todo!()
    }

    /// Create tables assets, device_seen, live_pairs, sidecars, imports if missing.
    pub fn migrate(&self) -> rusqlite::Result<()> {
        todo!()
    }

    pub fn has_asset(&self, hash: &str) -> rusqlite::Result<bool> {
        todo!()
    }

    pub fn insert_asset(&self, a: &AssetRow) -> rusqlite::Result<()> {
        todo!()
    }

    pub fn asset_path(&self, hash: &str) -> rusqlite::Result<Option<String>> {
        todo!()
    }

    pub fn seen(&self, device_id: &str, object_id: &str, size: u64) -> rusqlite::Result<Option<String>> {
        todo!()
    }

    pub fn mark_seen(&self, device_id: &str, object_id: &str, size: u64, hash: &str) -> rusqlite::Result<()> {
        todo!()
    }

    pub fn link_live_pair(&self, photo_hash: &str, mov_hash: &str) -> rusqlite::Result<()> {
        todo!()
    }

    pub fn link_sidecar(&self, asset_hash: &str, sidecar_hash: &str, library_path: &str) -> rusqlite::Result<()> {
        todo!()
    }

    /// Returns the new import id.
    pub fn begin_import(&self, source: &str) -> rusqlite::Result<i64> {
        todo!()
    }

    pub fn finish_import(&self, id: i64, copied: u64, skipped: u64, failed: u64, errors: &str) -> rusqlite::Result<()> {
        todo!()
    }
}
