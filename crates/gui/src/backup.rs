use std::path::PathBuf;

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
    /// Step index, a one-line summary, and the file count shown on that step's folder.
    Completed(usize, String, u64),
    /// Fraction of the whole backup done, from 0.0 to 1.0. Never decreases.
    Progress(f32),
    Failed(String),
    Finished,
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
