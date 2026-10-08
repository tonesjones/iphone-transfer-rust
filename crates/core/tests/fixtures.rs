use chrono::NaiveDate;
use photoxfer_core::metadata::capture_time;
use std::path::Path;

mod common;
use common::*;

#[test]
fn parses_exif_jpeg_and_missing_exif() {
    let dated = write_fixture(".jpg", &jpeg_with_date());
    let expected = NaiveDate::from_ymd_opt(2026, 10, 7)
        .and_then(|d| d.and_hms_opt(14, 30, 12))
        .expect("date");
    let parsed = nom_exif::read_exif(Path::new(&dated)).expect("fixture EXIF parse");
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
    assert_eq!(capture_time(Path::new(&dated)), Some(expected));
    let missing = write_fixture(".jpg", &jpeg_without_exif());
    assert_eq!(capture_time(Path::new(&missing)), None);
}

#[test]
fn parses_quicktime_creation_and_zero_epoch() {
    let expected = NaiveDate::from_ymd_opt(2026, 10, 7)
        .and_then(|d| d.and_hms_opt(14, 30, 12))
        .expect("date");
    let seconds = (expected.and_utc().timestamp()
        - NaiveDate::from_ymd_opt(1904, 1, 1)
            .expect("epoch")
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .timestamp()) as u32;
    let dated = write_fixture(".mov", &mov_with_creation_time(seconds));
    assert_eq!(capture_time(Path::new(&dated)), Some(expected));
    let zero = write_fixture(".mov", &mov_with_creation_time(0));
    assert_eq!(capture_time(Path::new(&zero)), None);
}

#[test]
fn invalid_datetime_returns_none() {
    let invalid = write_fixture(".jpg", &jpeg_with_datetime("not a datetime"));
    assert_eq!(capture_time(Path::new(&invalid)), None);
}

#[test]
fn quicktime_offset_preserves_camera_wall_time() {
    let expected = NaiveDate::from_ymd_opt(2026, 10, 7)
        .and_then(|d| d.and_hms_opt(14, 30, 12))
        .expect("date");
    for offset in ["+05:30", "-07:00", "+00:00"] {
        let bytes = mov_with_offset_datetime(&format!("2026-10-07T14:30:12{offset}"));
        let file = write_fixture(".mov", &bytes);
        assert_eq!(capture_time(Path::new(&file)), Some(expected), "{offset}");
    }
}
