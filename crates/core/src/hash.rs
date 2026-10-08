/// Full BLAKE3 hash as lowercase hex (64 chars). Streams the file; never loads it whole.
pub fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    hash_reader(std::fs::File::open(path)?)
}

/// Hash a reader in 64 KiB chunks.
pub fn hash_reader<R: std::io::Read>(mut r: R) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// First 6 hex chars of a full hash.
pub fn short_hash(full: &str) -> &str {
    &full[..6]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn hashes_known_input_and_large_reader() {
        let bytes = b"photoxfer";
        assert_eq!(
            hash_reader(Cursor::new(bytes)).expect("hash"),
            blake3::hash(bytes).to_hex().to_string()
        );
        let large = vec![42; 128 * 1024 + 17];
        assert_eq!(
            hash_reader(Cursor::new(&large)).expect("large hash"),
            blake3::hash(&large).to_hex().to_string()
        );
    }
    #[test]
    fn hashes_file_larger_than_64_kib() {
        let temp = tempfile::NamedTempFile::new().expect("temp");
        let bytes: Vec<u8> = (0..131_089).map(|i| (i % 251) as u8).collect();
        std::fs::write(temp.path(), &bytes).expect("write");
        assert_eq!(
            hash_file(temp.path()).expect("hash"),
            blake3::hash(&bytes).to_hex().to_string()
        );
    }
}
