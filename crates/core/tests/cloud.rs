use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use photoxfer_core::{
    cloud::{CloudFiles, CloudReport, PinState, cloud_test, cloud_test_with_timeout},
    hash,
};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Pin(PathBuf),
    Online(PathBuf),
    Evicted(PathBuf),
    Restore(PathBuf, PinState),
}

struct Fake {
    original: HashMap<PathBuf, PinState>,
    states: RefCell<HashMap<PathBuf, PinState>>,
    calls: RefCell<Vec<Call>>,
    refuse: HashSet<PathBuf>,
    pin_error: Option<PathBuf>,
    online_error: Option<PathBuf>,
    eviction_error: Option<PathBuf>,
    restore_errors: HashSet<PathBuf>,
}

impl Fake {
    fn new(paths: &[PathBuf]) -> Self {
        let original: HashMap<_, _> = paths
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    fs::canonicalize(p).unwrap(),
                    [PinState::Pinned, PinState::Unpinned, PinState::Neither][i % 3],
                )
            })
            .collect();
        Self {
            states: RefCell::new(original.clone()),
            original,
            calls: RefCell::new(Vec::new()),
            refuse: HashSet::new(),
            pin_error: None,
            online_error: None,
            eviction_error: None,
            restore_errors: HashSet::new(),
        }
    }

    fn selected(&self) -> Vec<PathBuf> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                Call::Online(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    fn assert_restored(&self) {
        assert_eq!(*self.states.borrow(), self.original);
        let selected = self.selected();
        let restored: Vec<_> = self
            .calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                Call::Restore(p, state) => {
                    assert_eq!(Some(state), self.original.get(p));
                    Some(p.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(restored, selected.into_iter().rev().collect::<Vec<_>>());
    }
}

impl CloudFiles for Fake {
    fn pin_state(&self, p: &Path) -> anyhow::Result<PinState> {
        self.calls.borrow_mut().push(Call::Pin(p.into()));
        if self.pin_error.as_deref() == Some(p) {
            anyhow::bail!("pin read failed");
        }
        Ok(self.states.borrow()[p])
    }

    fn make_online_only(&self, p: &Path) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(Call::Online(p.into()));
        self.states
            .borrow_mut()
            .insert(p.into(), PinState::Unpinned);
        if self.online_error.as_deref() == Some(p) {
            anyhow::bail!("online-only failed after changing attributes");
        }
        Ok(())
    }

    fn is_evicted(&self, p: &Path) -> anyhow::Result<bool> {
        self.calls.borrow_mut().push(Call::Evicted(p.into()));
        if self.eviction_error.as_deref() == Some(p) {
            anyhow::bail!("eviction query failed");
        }
        Ok(!self.refuse.contains(p))
    }

    fn set_pin_state(&self, p: &Path, s: PinState) -> anyhow::Result<()> {
        self.calls.borrow_mut().push(Call::Restore(p.into(), s));
        if self.restore_errors.contains(p) {
            anyhow::bail!("restore failed");
        }
        self.states.borrow_mut().insert(p.into(), s);
        Ok(())
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    onedrive: PathBuf,
    archive: PathBuf,
    manifest: PathBuf,
    paths: Vec<PathBuf>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let onedrive = temp.path().join("OneDrive");
        let archive = onedrive.join("Photo Backups");
        fs::create_dir_all(archive.join("manifests")).unwrap();
        fs::create_dir(archive.join("media")).unwrap();
        fs::create_dir(archive.join("catalog-backups")).unwrap();
        let mut text = String::from("photoxfer-blake3-v1\n");
        let mut paths = Vec::new();
        for relative in (0..5)
            .map(|i| format!("media/{i}.jpg"))
            .chain(["catalog-backups/library-001.db".into()])
        {
            let path = archive.join(&relative);
            fs::write(&path, format!("fixture bytes for {relative}")).unwrap();
            text.push_str(&format!(
                "{}\t{}\t{relative}\n",
                hash::hash_file(&path).unwrap(),
                fs::metadata(&path).unwrap().len()
            ));
            paths.push(fs::canonicalize(path).unwrap());
        }
        text.push_str("complete\t6\n");
        let manifest = archive.join("manifests/library-001.tsv");
        fs::write(&manifest, text).unwrap();
        fs::write(archive.join("notes.txt"), b"unrecorded, untouched").unwrap();
        Self {
            _temp: temp,
            onedrive,
            archive,
            manifest,
            paths,
        }
    }

    fn run(&self, fake: &Fake, sample: usize, seed: u64) -> anyhow::Result<CloudReport> {
        cloud_test_with_timeout(
            &self.archive,
            Some(&self.onedrive),
            sample,
            seed,
            fake,
            &mut |_| {},
            Duration::ZERO,
        )
    }
}

fn contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            result.extend(contents(&entry.path()));
        } else {
            result.insert(entry.path(), fs::read(entry.path()).unwrap());
        }
    }
    result
}

#[test]
fn passes_always_samples_snapshot_and_reports_read_progress() {
    let f = Fixture::new();
    let fake = Fake::new(&f.paths);
    let before = contents(&f.archive);
    let mut progress = Vec::new();
    let report = cloud_test(&f.archive, Some(&f.onedrive), 2, 42, &fake, &mut |done| {
        progress.push(done)
    })
    .unwrap();
    assert_eq!(report.files, 3);
    assert_eq!(report.manifest, "library-001.tsv");
    assert_eq!(fake.selected().last(), f.paths.last());
    assert_eq!(progress, vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);
    let calls = fake.calls.borrow();
    let first_eviction = calls
        .iter()
        .position(|c| matches!(c, Call::Evicted(_)))
        .unwrap();
    assert_eq!(calls[..first_eviction].len(), 6);
    drop(calls);
    fake.assert_restored();
    assert_eq!(contents(&f.archive), before);
}

#[test]
fn never_evicting_files_are_all_named_and_pins_restored() {
    let f = Fixture::new();
    let mut fake = Fake::new(&f.paths);
    fake.refuse.extend([f.paths[0].clone(), f.paths[5].clone()]);
    let before = contents(&f.archive);
    let error = f.run(&fake, 25, 42).unwrap_err().to_string();
    assert!(error.contains("not uploaded to OneDrive yet"));
    assert!(error.contains("media/0.jpg"));
    assert!(error.contains("catalog-backups/library-001.db"));
    fake.assert_restored();
    assert_eq!(contents(&f.archive), before);
}

#[test]
fn corrupted_file_is_named_and_pins_restored() {
    let f = Fixture::new();
    let bytes = fs::read(&f.paths[0]).unwrap();
    fs::write(&f.paths[0], vec![b'x'; bytes.len()]).unwrap();
    let before = contents(&f.archive);
    let fake = Fake::new(&f.paths);
    let error = f.run(&fake, 25, 42).unwrap_err().to_string();
    assert!(error.contains("cloud verification failed: media/0.jpg"));
    fake.assert_restored();
    assert_eq!(contents(&f.archive), before);
}

#[test]
fn wrong_size_is_named_and_pins_restored() {
    let f = Fixture::new();
    fs::write(&f.paths[5], b"short").unwrap();
    let fake = Fake::new(&f.paths);
    let error = f.run(&fake, 25, 42).unwrap_err().to_string();
    assert!(error.contains("cloud verification failed: catalog-backups/library-001.db"));
    fake.assert_restored();
}

#[test]
fn all_three_original_pin_states_are_restored_exactly() {
    let f = Fixture::new();
    let fake = Fake::new(&f.paths);
    assert_eq!(f.run(&fake, 25, 42).unwrap().files, 6);
    for state in [PinState::Pinned, PinState::Unpinned, PinState::Neither] {
        assert!(fake.original.values().any(|s| *s == state));
    }
    fake.assert_restored();
}

#[test]
fn outside_onedrive_and_missing_root_refused_before_touching_files() {
    let f = Fixture::new();
    let unrelated = tempfile::tempdir().unwrap();
    let fake = Fake::new(&f.paths);
    for root in [None, Some(unrelated.path())] {
        let error = cloud_test(&f.archive, root, 25, 42, &fake, &mut |_| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("cloud test needs the archive inside OneDrive"));
        assert!(error.contains("Photo Backups"));
    }
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn sampling_matches_seed_ranking_is_repeatable_and_respects_n() {
    let f = Fixture::new();
    for seed in [0_u64, 42, u64::MAX] {
        let mut expected = f.paths[..5].to_vec();
        expected.sort_by_cached_key(|path| {
            let mut bytes = seed.to_le_bytes().to_vec();
            bytes.extend(hash::hash_file(path).unwrap().as_bytes());
            *blake3::hash(&bytes).as_bytes()
        });
        for sample in [0, 1, 3, 25] {
            let first = Fake::new(&f.paths);
            let second = Fake::new(&f.paths);
            assert_eq!(
                f.run(&first, sample, seed).unwrap().files,
                sample.min(5) + 1
            );
            f.run(&second, sample, seed).unwrap();
            let mut selected = expected[..sample.min(5)].to_vec();
            selected.push(f.paths[5].clone());
            assert_eq!(first.selected(), selected);
            assert_eq!(second.selected(), selected);
            first.assert_restored();
            second.assert_restored();
        }
    }
}

#[test]
fn malformed_manifests_refused_before_touching_files() {
    let f = Fixture::new();
    for text in [
        "wrong-header\ncomplete\t0\n",
        "photoxfer-blake3-v1\n",
        "photoxfer-blake3-v1\ncomplete\t2\n",
        "photoxfer-blake3-v1\ncomplete\t0\n",
        "photoxfer-blake3-v1\ninvalid\ncomplete\t1\n",
        "photoxfer-blake3-v1\nabc\t3\t../outside.jpg\ncomplete\t1\n",
        "photoxfer-blake3-v1\nabc\t3\tmissing.jpg\ncomplete\t1\n",
        "photoxfer-blake3-v1\nabc\t3\tmedia/0.jpg\ncomplete\t1\n",
    ] {
        fs::write(&f.manifest, text).unwrap();
        let fake = Fake::new(&f.paths);
        assert!(f.run(&fake, 25, 42).is_err(), "{text}");
        assert!(fake.calls.borrow().is_empty());
    }
}

#[test]
fn attribute_errors_restore_all_previously_touched_files() {
    let f = Fixture::new();
    for operation in ["pin", "online", "eviction"] {
        let mut fake = Fake::new(&f.paths);
        let snapshot = Some(f.paths[5].clone());
        match operation {
            "pin" => fake.pin_error = snapshot,
            "online" => fake.online_error = snapshot,
            _ => fake.eviction_error = snapshot,
        }
        let error = format!("{:#}", f.run(&fake, 25, 42).unwrap_err());
        assert!(error.contains("catalog-backups/library-001.db"));
        assert!(error.contains("failed"));
        fake.assert_restored();
    }
}

#[test]
fn restore_errors_are_reported_without_hiding_original_error() {
    let f = Fixture::new();
    for fail_test in [false, true] {
        let mut fake = Fake::new(&f.paths);
        fake.restore_errors
            .extend([f.paths[0].clone(), f.paths[5].clone()]);
        if fail_test {
            fake.refuse.insert(f.paths[1].clone());
        }
        let error = format!("{:#}", f.run(&fake, 25, 42).unwrap_err());
        assert!(error.contains("Could not restore pin states"));
        assert!(error.contains("0.jpg"));
        assert!(error.contains("library-001.db"));
        if fail_test {
            assert!(error.contains("not uploaded to OneDrive yet: media/1.jpg"));
        }
        assert_eq!(
            fake.calls
                .borrow()
                .iter()
                .filter(|c| matches!(c, Call::Restore(..)))
                .count(),
            6
        );
    }
}

#[test]
fn unwinding_progress_callback_restores_pins() {
    let f = Fixture::new();
    let fake = Fake::new(&f.paths);
    let before = contents(&f.archive);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cloud_test(&f.archive, Some(&f.onedrive), 25, 42, &fake, &mut |_| {
            panic!("callback panicked")
        })
    }));
    assert!(result.is_err());
    fake.assert_restored();
    assert_eq!(contents(&f.archive), before);
}
