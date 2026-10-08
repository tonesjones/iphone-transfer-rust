// JPEG bytes: SOI, one APP1 Exif segment whose little-endian TIFF header points
// IFD0 links to an ExifIFD containing DateTimeOriginal, then a minimal
// SOF0/SOS sequence and EOI. The pixel payload is intentionally empty.
pub fn jpeg_with_date() -> Vec<u8> {
    jpeg_with_datetime("2026:10:07 14:30:12")
}

pub fn jpeg_with_datetime(datetime: &str) -> Vec<u8> {
    let mut date = datetime.as_bytes().to_vec();
    date.push(0);
    let exif_ifd = 26;
    let data_offset = 44;
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2a\0\x08\0\0\0");
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x8769u16.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&(exif_ifd as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x9003u16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&(date.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&(data_offset as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&date);
    let mut jpeg = vec![0xff, 0xd8];
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    jpeg.extend_from_slice(&[0xff, 0xe1]);
    jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(&payload);
    // One-component baseline frame and scan headers are sufficient to delimit a valid JPEG.
    jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x0b, 8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    jpeg.extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 1, 1, 0, 0, 0x3f, 0]);
    jpeg.push(0); // one tiny scan byte, then EOI
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg.resize(9000, 0); // parser's initial read is 8 KiB; ensure it can refill to EOF.
    jpeg
}

pub fn jpeg_without_exif() -> Vec<u8> {
    vec![0xff, 0xd8, 0xff, 0xd9]
}

// MOV bytes: QuickTime ftyp followed by moov/mvhd. Version 0 mvhd has a
// 32-bit creation timestamp at seconds since 1904-01-01, then the fixed fields.
pub fn mov_with_creation_time(seconds: u32) -> Vec<u8> {
    let mut mvhd = Vec::new();
    mvhd.extend_from_slice(&[0, 0, 0, 0]); // version and flags
    mvhd.extend_from_slice(&seconds.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes()); // modification time
    mvhd.extend_from_slice(&1000u32.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes()); // duration
    mvhd.extend_from_slice(&[0; 76]);
    mvhd.extend_from_slice(&1u32.to_be_bytes()); // next track id
    let mut mvhd_box = Vec::new();
    mvhd_box.extend_from_slice(&((mvhd.len() + 8) as u32).to_be_bytes());
    mvhd_box.extend_from_slice(b"mvhd");
    mvhd_box.extend_from_slice(&mvhd);
    let mut moov = Vec::new();
    moov.extend_from_slice(&((mvhd_box.len() + 8) as u32).to_be_bytes());
    moov.extend_from_slice(b"moov");
    moov.extend_from_slice(&mvhd_box);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&20u32.to_be_bytes());
    bytes.extend_from_slice(b"ftypqt  ");
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(b"qt  ");
    bytes.extend_from_slice(&moov);
    bytes
}

pub fn write_fixture(ext: &str, bytes: &[u8]) -> tempfile::TempPath {
    let file = tempfile::Builder::new()
        .suffix(ext)
        .tempfile()
        .expect("fixture file");
    std::fs::write(file.path(), bytes).expect("fixture bytes");
    file.into_temp_path()
}

// moov/meta contains keys (one mdta key) and ilst (one UTF-8 data item).
// Its creationdate takes precedence over the mvhd timestamp, including its offset.
pub fn mov_with_offset_datetime(datetime: &str) -> Vec<u8> {
    fn atom(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(body);
        bytes
    }
    let key = b"com.apple.quicktime.creationdate";
    let mut keys = vec![0; 4]; // version/flags
    keys.extend_from_slice(&1u32.to_be_bytes()); // entry count
    keys.extend_from_slice(&atom(b"mdta", key));
    let mut data = 1u32.to_be_bytes().to_vec(); // UTF-8 type indicator
    data.extend_from_slice(&0u32.to_be_bytes()); // locale
    data.extend_from_slice(datetime.as_bytes());
    let item = atom(&1u32.to_be_bytes(), &atom(b"data", &data));
    let mut meta = atom(b"keys", &keys);
    meta.extend_from_slice(&atom(b"ilst", &item));
    let mut bytes = mov_with_creation_time(0);
    let meta = atom(b"meta", &meta);
    let moov_size = u32::from_be_bytes(bytes[20..24].try_into().expect("moov size"));
    bytes[20..24].copy_from_slice(&(moov_size + meta.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&meta);
    bytes
}
