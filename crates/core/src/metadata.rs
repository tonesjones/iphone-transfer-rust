use chrono::{NaiveDate, NaiveDateTime};

const QUICKTIME_EPOCH: Option<NaiveDateTime> = match NaiveDate::from_ymd_opt(1904, 1, 1) {
    Some(date) => date.and_hms_opt(0, 0, 0),
    None => None,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
    Sidecar,
    Other,
}

/// Classify by lowercase extension: heic/heif/jpg/jpeg/png/dng → Photo; mov/mp4/m4v → Video; aae → Sidecar.
pub fn kind_for(path: &std::path::Path) -> MediaKind {
    match path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("heic" | "heif" | "jpg" | "jpeg" | "png" | "dng") => MediaKind::Photo,
        Some("mov" | "mp4" | "m4v") => MediaKind::Video,
        Some("aae") => MediaKind::Sidecar,
        _ => MediaKind::Other,
    }
}

/// Capture time from EXIF DateTimeOriginal (photos) or QuickTime creation date (videos).
/// None when the file has no usable date.
pub fn capture_time(path: &std::path::Path) -> Option<NaiveDateTime> {
    use nom_exif::{EntryValue, ExifTag, TrackInfoTag};

    match kind_for(path) {
        MediaKind::Photo => {
            let exif = nom_exif::read_exif(path).ok()?;
            // Errors on unrelated tags are common in real files; only the date tags matter.
            [
                ExifTag::DateTimeOriginal,
                ExifTag::CreateDate,
                ExifTag::ModifyDate,
            ]
            .into_iter()
            .find_map(|tag| exif.get(tag).and_then(EntryValue::as_datetime))
            .map(|dt| dt.into_naive())
        }
        MediaKind::Video => {
            let track = nom_exif::read_track(path).ok()?;
            let datetime = track.get(TrackInfoTag::CreateDate)?.as_datetime()?;
            // nom-exif prefers Apple's offset-bearing metadata over mvhd (mov.rs:31-34).
            // mvhd is emitted with offset zero; naive_local therefore also gives UTC there.
            let (utc, local) = match datetime {
                nom_exif::ExifDateTime::Aware(dt) => (dt.naive_utc(), dt.naive_local()),
                nom_exif::ExifDateTime::Naive(dt) => (dt, dt),
            };
            if Some(utc) == QUICKTIME_EPOCH {
                return None;
            }
            Some(local)
        }
        MediaKind::Sidecar | MediaKind::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_extensions_case_insensitively() {
        assert_eq!(kind_for(std::path::Path::new("X.HEIC")), MediaKind::Photo);
        assert_eq!(kind_for(std::path::Path::new("X.MOV")), MediaKind::Video);
        assert_eq!(kind_for(std::path::Path::new("X.AAE")), MediaKind::Sidecar);
        assert_eq!(kind_for(std::path::Path::new("X.txt")), MediaKind::Other);
    }
}
