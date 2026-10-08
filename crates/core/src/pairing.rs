use std::collections::HashMap;
use std::path::PathBuf;

/// One import unit: a primary file plus its Live Photo MOV and AAE sidecar if present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub primary: PathBuf,
    pub live_mov: Option<PathBuf>,
    pub sidecar: Option<PathBuf>,
}

/// Group files by (parent dir, case-insensitive base name). A HEIC/JPG with a same-named MOV
/// becomes a Live Photo group. An AAE attaches to the same-named photo. `IMG_E1234` is its own
/// base name and its own group. A MOV/AAE with no photo partner becomes its own primary group.
/// Output is sorted by primary path.
pub fn group_files(files: &[PathBuf]) -> Vec<Group> {
    use crate::metadata::{MediaKind, kind_for};
    let key = |path: &PathBuf| {
        let parent = path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .to_path_buf();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        (parent, stem)
    };
    let mut photos: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::new();
    let mut videos: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::new();
    let mut sidecars: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::new();
    let mut others = Vec::new();
    for file in files {
        match kind_for(file) {
            MediaKind::Photo => photos.entry(key(file)).or_default().push(file.clone()),
            MediaKind::Video => videos.entry(key(file)).or_default().push(file.clone()),
            MediaKind::Sidecar => sidecars.entry(key(file)).or_default().push(file.clone()),
            MediaKind::Other => others.push(file.clone()),
        }
    }
    for entries in photos
        .values_mut()
        .chain(videos.values_mut())
        .chain(sidecars.values_mut())
    {
        entries.sort();
    }

    let mut output = Vec::new();
    for (k, photo_files) in &photos {
        // When extensions collide, iOS's HEIC is the Live Photo original; JPG remains standalone.
        let preferred = photo_files
            .iter()
            .position(|p| {
                p.extension()
                    .map(|e| e.to_string_lossy().eq_ignore_ascii_case("heic"))
                    .unwrap_or(false)
            })
            .unwrap_or(0);
        for (i, photo) in photo_files.iter().enumerate() {
            let attached = i == preferred;
            let live_mov = if attached {
                videos.get_mut(k).and_then(|v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.remove(0))
                    }
                })
            } else {
                None
            };
            let sidecar = if attached {
                sidecars.get_mut(k).and_then(|v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.remove(0))
                    }
                })
            } else {
                None
            };
            output.push(Group {
                primary: photo.clone(),
                live_mov,
                sidecar,
            });
        }
    }
    for primary in videos.into_values().chain(sidecars.into_values()).flatten() {
        output.push(Group {
            primary,
            live_mov: None,
            sidecar: None,
        });
    }
    output.extend(others.into_iter().map(|primary| Group {
        primary,
        live_mov: None,
        sidecar: None,
    }));
    output.sort_by(|a, b| a.primary.cmp(&b.primary));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pairs_and_preserves_every_input_once() {
        let files: Vec<PathBuf> = [
            "a/IMG_1.heic",
            "a/IMG_1.JPG",
            "a/IMG_1.MOV",
            "a/IMG_1.AAE",
            "a/IMG_E1.jpg",
            "a/IMG_E1.MOV",
            "b/IMG_1.heic",
            "b/IMG_1.MOV",
            "orphan.MOV",
            "orphan.AAE",
            "other.bin",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        let groups = group_files(&files);
        let mut counts: HashMap<PathBuf, usize> = HashMap::new();
        for group in &groups {
            *counts.entry(group.primary.clone()).or_default() += 1;
            if let Some(p) = &group.live_mov {
                *counts.entry(p.clone()).or_default() += 1;
            }
            if let Some(p) = &group.sidecar {
                *counts.entry(p.clone()).or_default() += 1;
            }
        }
        for file in files {
            assert_eq!(counts.get(&file), Some(&1), "{file:?}");
        }
        let heic = groups
            .iter()
            .find(|g| g.primary.as_path() == std::path::Path::new("a/IMG_1.heic"))
            .expect("heic");
        assert_eq!(heic.live_mov, Some(PathBuf::from("a/IMG_1.MOV")));
        assert_eq!(heic.sidecar, Some(PathBuf::from("a/IMG_1.AAE")));
        let jpg = groups
            .iter()
            .find(|g| g.primary.as_path() == std::path::Path::new("a/IMG_1.JPG"))
            .expect("jpg");
        assert!(jpg.live_mov.is_none() && jpg.sidecar.is_none());
    }
    fn groups(paths: &[&str]) -> Vec<Group> {
        group_files(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
    }
    fn expected(primary: &str, mov: Option<&str>, aae: Option<&str>) -> Group {
        Group {
            primary: primary.into(),
            live_mov: mov.map(PathBuf::from),
            sidecar: aae.map(PathBuf::from),
        }
    }
    #[test]
    fn live_pair() {
        assert_eq!(
            groups(&["IMG_1.HEIC", "IMG_1.MOV"]),
            vec![expected("IMG_1.HEIC", Some("IMG_1.MOV"), None)]
        );
    }
    #[test]
    fn aae_sidecar() {
        assert_eq!(
            groups(&["IMG_1.jpg", "IMG_1.aae"]),
            vec![expected("IMG_1.jpg", None, Some("IMG_1.aae"))]
        );
    }
    #[test]
    fn img_e_edit_is_separate() {
        assert_eq!(
            groups(&["IMG_1.jpg", "IMG_E1.jpg", "IMG_1.MOV"]),
            vec![
                expected("IMG_1.jpg", Some("IMG_1.MOV"), None),
                expected("IMG_E1.jpg", None, None)
            ]
        );
    }
    #[test]
    fn case_insensitive_pairing() {
        assert_eq!(
            groups(&["IMG_1.heic", "img_1.MOV"]),
            vec![expected("IMG_1.heic", Some("img_1.MOV"), None)]
        );
    }
    #[test]
    fn same_stem_different_directories() {
        assert_eq!(
            groups(&["a/IMG_1.heic", "A/IMG_1.MOV"]),
            vec![
                expected("A/IMG_1.MOV", None, None),
                expected("a/IMG_1.heic", None, None)
            ]
        );
    }
    #[test]
    fn orphan_mov() {
        assert_eq!(
            groups(&["orphan.MOV"]),
            vec![expected("orphan.MOV", None, None)]
        );
    }
    #[test]
    fn orphan_aae() {
        assert_eq!(
            groups(&["orphan.AAE"]),
            vec![expected("orphan.AAE", None, None)]
        );
    }
    #[test]
    fn heic_receives_mov_and_aae_over_jpg() {
        assert_eq!(
            groups(&["IMG_1.JPG", "IMG_1.HEIC", "IMG_1.MOV", "IMG_1.AAE"]),
            vec![
                expected("IMG_1.HEIC", Some("IMG_1.MOV"), Some("IMG_1.AAE")),
                expected("IMG_1.JPG", None, None)
            ]
        );
    }
    #[test]
    fn output_sorted_by_primary() {
        assert_eq!(
            groups(&["z.jpg", "b.MOV", "a.heic"]),
            vec![
                expected("a.heic", None, None),
                expected("b.MOV", None, None),
                expected("z.jpg", None, None)
            ]
        );
    }
    #[test]
    fn repeated_input_paths_are_preserved() {
        assert_eq!(
            groups(&["p.heic", "p.mov", "p.mov", "p.aae", "p.aae"]),
            vec![
                expected("p.aae", None, None),
                expected("p.heic", Some("p.mov"), Some("p.aae")),
                expected("p.mov", None, None),
            ]
        );
    }
}
