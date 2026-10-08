use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
    Sidecar,
    Other,
}

/// Classify by lowercase extension: heic/heif/jpg/jpeg/png/dng → Photo; mov/mp4/m4v → Video; aae → Sidecar.
pub fn kind_for(path: &std::path::Path) -> MediaKind {
    todo!()
}

/// Capture time from EXIF DateTimeOriginal (photos) or QuickTime creation date (videos).
/// None when the file has no usable date.
pub fn capture_time(path: &std::path::Path) -> Option<NaiveDateTime> {
    todo!()
}
