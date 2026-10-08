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
    todo!()
}
