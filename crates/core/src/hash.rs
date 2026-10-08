/// Full BLAKE3 hash as lowercase hex (64 chars). Streams the file; never loads it whole.
pub fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    todo!()
}

/// Hash a reader in 64 KiB chunks.
pub fn hash_reader<R: std::io::Read>(r: R) -> std::io::Result<String> {
    todo!()
}

/// First 6 hex chars of a full hash.
pub fn short_hash(full: &str) -> &str {
    &full[..6]
}
