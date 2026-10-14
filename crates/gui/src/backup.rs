use std::path::{Path, PathBuf};

use anyhow::bail;
use photoxfer_core::{archive, check, import};

#[derive(Clone)]
pub struct Folders {
    pub source: PathBuf,
    pub library: PathBuf,
    pub archive: PathBuf,
}

pub enum Event {
    Started(usize),
    /// Library paths of files the save step copied, for spot checks.
    Saved(Vec<PathBuf>),
    /// Step index, a one-line summary, and the file count shown on that step's folder.
    Completed(usize, String, u64),
    /// Fraction of the whole backup done, from 0.0 to 1.0. Never decreases.
    Progress(f32),
    Failed(String),
    Finished,
}

pub(crate) enum RestoreEvent {
    Progress(f32),
    Passed { files: usize, manifest: String },
    Failed(String),
}

pub(crate) fn refuse_restore_folder(
    folder: &Path,
    folders: &Folders,
    onedrive: Option<&Path>,
) -> Option<String> {
    let folder = match std::fs::canonicalize(folder) {
        Ok(path) => path,
        Err(error) => return Some(format!("Could not open the downloaded folder: {error}")),
    };
    let blocked = [
        Some(folders.archive.as_path()),
        Some(folders.library.as_path()),
        onedrive,
    ];
    for root in blocked.into_iter().flatten() {
        match std::fs::canonicalize(root) {
            Ok(root) if folder.starts_with(&root) => {
                return Some(format!(
                    "{} is on this computer's OneDrive or backup folders, so it doesn't test the cloud copy. Download to a folder outside OneDrive, such as C:\\PhotoXfer-Restore.",
                    display(&folder)
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Some(format!(
                    "Could not check the backup folder {}: {error}",
                    root.display()
                ));
            }
        }
    }
    None
}

/// Drops the `\\?\` prefix that `canonicalize` adds on Windows.
fn display(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text)
}

pub(crate) fn run_restore(folder: &Path, manifest: &Path, mut report: impl FnMut(RestoreEvent)) {
    match archive::verify_with_progress(folder, manifest, &mut |done| {
        report(RestoreEvent::Progress(done));
    }) {
        Ok(files) => report(RestoreEvent::Passed {
            files,
            manifest: manifest
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        }),
        Err(error) => report(RestoreEvent::Failed(format!("{error:#}"))),
    }
}

/// Each step's share of the progress bar. The archive step reads every file about twice as
/// often as the others (see `archive::archive_with_progress`).
const WEIGHTS: [f32; 3] = [0.25, 0.25, 0.5];

fn overall(step: usize, done: f32) -> Event {
    let before: f32 = WEIGHTS[..step].iter().sum();
    Event::Progress(before + WEIGHTS[step] * done.clamp(0.0, 1.0))
}

pub fn run(folders: &Folders, mut report: impl FnMut(Event)) {
    let result = (|| -> anyhow::Result<()> {
        report(Event::Started(0));
        let saved =
            import::import_folder_with_progress(&folders.source, &folders.library, &mut |done| {
                report(overall(0, done))
            })?;
        if saved.failed != 0 || !saved.errors.is_empty() {
            bail!(
                "Could not save {} source files or folders.\n{}",
                saved.failed.max(saved.errors.len() as u64),
                saved.errors.join("\n")
            );
        }
        report(Event::Saved(saved.saved));
        report(Event::Completed(
            0,
            format!(
                "{} newly saved · {} already saved",
                saved.copied, saved.skipped
            ),
            saved.found,
        ));
        report(Event::Started(1));
        let checked = check::check_with_progress(&folders.source, &folders.library, &mut |done| {
            report(overall(1, done))
        })?;
        if !checked.is_clean() {
            let mut problems = checked.library_problems;
            problems.extend(checked.errors);
            problems.extend(
                checked
                    .missing
                    .iter()
                    .map(|p| format!("Not backed up: {}", p.display())),
            );
            problems.extend(
                checked
                    .untracked
                    .iter()
                    .map(|p| format!("Not in catalog: {}", p.display())),
            );
            bail!(
                "Your saved library needs attention.\n{}",
                problems.join("\n")
            );
        }
        report(Event::Completed(
            1,
            format!(
                "{} files verified · covers all {} source files",
                checked.verified_library_files,
                checked.safe.len()
            ),
            checked.verified_library_files as u64,
        ));
        report(Event::Started(2));
        let second =
            archive::archive_with_progress(&folders.library, &folders.archive, &mut |done| {
                report(overall(2, done))
            })?;
        report(Event::Completed(
            2,
            format!(
                "{} files verified · {} newly copied",
                second.verified, second.copied
            ),
            second.verified as u64,
        ));
        Ok(())
    })();
    match result {
        Ok(()) => report(Event::Finished),
        Err(error) => report(Event::Failed(format!("{error:#}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(root: &std::path::Path) -> Folders {
        Folders {
            source: root.join("source"),
            library: root.join("library"),
            archive: root.join("archive"),
        }
    }

    #[test]
    fn restore_folder_refusals() {
        let local = tempfile::tempdir().unwrap();
        let unrelated = tempfile::tempdir().unwrap();
        let mut f = folders(local.path());
        f.archive = local.path().join("backups").join("archive");
        let inside_archive = f.archive.join("download");
        let onedrive = local.path().join("OneDrive");
        let inside_onedrive = onedrive.join("download");
        for path in [&inside_archive, &f.library, &inside_onedrive] {
            std::fs::create_dir_all(path).unwrap();
        }
        for path in [&inside_archive, &f.library, &inside_onedrive] {
            assert!(refuse_restore_folder(path, &f, Some(&onedrive)).is_some());
        }
        assert!(refuse_restore_folder(unrelated.path(), &f, Some(&onedrive)).is_none());
    }

    fn copy_folder(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_folder(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn restored_download_passes_then_names_missing_media() {
        let local = tempfile::tempdir().unwrap();
        let restored = tempfile::tempdir().unwrap();
        let f = folders(local.path());
        std::fs::create_dir(&f.source).unwrap();
        std::fs::write(f.source.join("IMG_1234.PNG"), b"photo fixture").unwrap();
        import::import_folder(&f.source, &f.library).unwrap();
        let archived = archive::archive(&f.library, &f.archive).unwrap();
        copy_folder(&f.archive, restored.path());
        let manifest = archive::latest_manifest(restored.path()).unwrap();
        let mut events = Vec::new();
        run_restore(restored.path(), &manifest, |event| events.push(event));
        assert!(
            matches!(events.last(), Some(RestoreEvent::Passed { files: 2, manifest: name }) if name == &archived.manifest.file_name().unwrap().to_string_lossy())
        );
        assert!(
            matches!(events.iter().rev().nth(1), Some(RestoreEvent::Progress(done)) if *done == 1.0)
        );
        let db = photoxfer_core::db::Db::open_read_only(&f.library.join("library.db")).unwrap();
        let (_, media) = db.recorded_files().unwrap().remove(0);
        drop(db);
        std::fs::remove_file(restored.path().join(&media)).unwrap();
        events.clear();
        run_restore(restored.path(), &manifest, |event| events.push(event));
        assert!(
            matches!(events.last(), Some(RestoreEvent::Failed(error)) if error.contains(&media))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, RestoreEvent::Passed { .. }))
        );
    }

    #[test]
    fn repeats_and_restores_missing_library_copy() {
        let temp = tempfile::tempdir().unwrap();
        let f = folders(temp.path());
        std::fs::create_dir(&f.source).unwrap();
        std::fs::write(f.source.join("IMG_1234.PNG"), b"photo fixture").unwrap();
        for _ in 0..2 {
            let mut events = Vec::new();
            run(&f, |event| events.push(event));
            assert!(matches!(events.last(), Some(Event::Finished)));
        }
        let db = photoxfer_core::db::Db::open_read_only(&f.library.join("library.db")).unwrap();
        let (_, path) = db.recorded_files().unwrap().remove(0);
        drop(db);
        std::fs::remove_file(f.library.join(&path)).unwrap();
        let mut events = Vec::new();
        run(&f, |event| events.push(event));
        assert!(matches!(events.last(), Some(Event::Finished)));
        assert_eq!(
            std::fs::read(f.library.join(path)).unwrap(),
            b"photo fixture"
        );
    }

    #[test]
    fn progress_rises_to_complete() {
        let temp = tempfile::tempdir().unwrap();
        let f = folders(temp.path());
        std::fs::create_dir(&f.source).unwrap();
        for name in ["IMG_0001.PNG", "IMG_0002.PNG", "IMG_0003.MOV"] {
            std::fs::write(f.source.join(name), name).unwrap();
        }
        let mut progress = Vec::new();
        run(&f, |event| {
            if let Event::Progress(done) = event {
                progress.push(done);
            }
        });
        assert!(progress.len() > 9);
        assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(progress.last(), Some(&1.0));
    }

    #[test]
    fn damaged_copy_stops_before_archive_and_never_reports_success() {
        let temp = tempfile::tempdir().unwrap();
        let f = folders(temp.path());
        std::fs::create_dir(&f.source).unwrap();
        std::fs::write(f.source.join("IMG_1234.PNG"), b"photo fixture").unwrap();
        run(&f, |_| {});
        let db = photoxfer_core::db::Db::open_read_only(&f.library.join("library.db")).unwrap();
        let (_, path) = db.recorded_files().unwrap().remove(0);
        drop(db);
        std::fs::write(f.library.join(&path), b"damaged").unwrap();
        let mut events = Vec::new();
        run(&f, |event| events.push(event));
        assert!(matches!(events.last(), Some(Event::Failed(_))));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::Started(2) | Event::Finished))
        );
        assert_eq!(std::fs::read(f.library.join(path)).unwrap(), b"damaged");
    }
}
