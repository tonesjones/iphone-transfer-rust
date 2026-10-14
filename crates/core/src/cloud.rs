use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};

use crate::{archive, hash};

#[derive(Debug)]
pub struct CloudReport {
    pub files: usize,
    pub manifest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinState {
    Pinned,
    Unpinned,
    Neither,
}

pub trait CloudFiles {
    fn pin_state(&self, p: &Path) -> anyhow::Result<PinState>;
    fn make_online_only(&self, p: &Path) -> anyhow::Result<()>;
    fn is_evicted(&self, p: &Path) -> anyhow::Result<bool>;
    fn set_pin_state(&self, p: &Path, s: PinState) -> anyhow::Result<()>;
}

struct RestorePins<'a> {
    files: &'a dyn CloudFiles,
    touched: Vec<(PathBuf, PinState)>,
}

impl RestorePins<'_> {
    fn restore(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for (path, state) in self.touched.drain(..).rev() {
            if let Err(error) = self.files.set_pin_state(&path, state) {
                errors.push(format!("{}: {error:#}", path.display()));
            }
        }
        errors
    }
}

impl Drop for RestorePins<'_> {
    fn drop(&mut self) {
        // Also restore if a progress callback unwinds.
        let _ = self.restore();
    }
}

pub fn cloud_test(
    archive: &Path,
    onedrive: Option<&Path>,
    sample: usize,
    seed: u64,
    files: &dyn CloudFiles,
    progress: &mut dyn FnMut(f32),
) -> anyhow::Result<CloudReport> {
    cloud_test_with_timeout(
        archive,
        onedrive,
        sample,
        seed,
        files,
        progress,
        Duration::from_secs(60),
    )
}

/// [`cloud_test`] with a bounded eviction wait, also used by fake-file tests.
pub fn cloud_test_with_timeout(
    archive: &Path,
    onedrive: Option<&Path>,
    sample: usize,
    seed: u64,
    files: &dyn CloudFiles,
    progress: &mut dyn FnMut(f32),
    timeout: Duration,
) -> anyhow::Result<CloudReport> {
    let refusal = || {
        format!(
            "{}: the cloud test needs the archive inside OneDrive",
            archive.display()
        )
    };
    let onedrive = onedrive.context(refusal())?;
    let root = fs::canonicalize(archive).with_context(refusal)?;
    let onedrive = fs::canonicalize(onedrive).with_context(refusal)?;
    if !root.starts_with(&onedrive) {
        bail!("{}", refusal());
    }
    let manifest = archive::latest_manifest(&root).context("No recovery file list found")?;
    let mut records = archive::manifest_lines(&manifest)?
        .iter()
        .map(|line| archive::manifest_record(&root, line))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let snapshot = records.pop().context("manifest contains no files")?;
    if !Path::new(&snapshot.relative).starts_with("catalog-backups") {
        bail!("manifest's last file is not a catalog snapshot");
    }
    records.sort_by_cached_key(|record| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        hasher.update(record.hash.as_bytes());
        *hasher.finalize().as_bytes()
    });
    records.truncate(sample);
    records.push(snapshot);

    let mut guard = RestorePins {
        files,
        touched: Vec::new(),
    };
    let result = (|| -> anyhow::Result<CloudReport> {
        let started = Instant::now();
        for record in &records {
            let state = files
                .pin_state(&record.path)
                .with_context(|| format!("Could not read pin state: {}", record.relative))?;
            // Record before changing attributes, including a call that partially fails.
            guard.touched.push((record.path.clone(), state));
            files
                .make_online_only(&record.path)
                .with_context(|| format!("Could not make online-only: {}", record.relative))?;
        }
        loop {
            let mut pending = Vec::new();
            for record in &records {
                if !files
                    .is_evicted(&record.path)
                    .with_context(|| format!("Could not check eviction: {}", record.relative))?
                {
                    pending.push(record.relative.as_str());
                }
            }
            if pending.is_empty() {
                break;
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                bail!("not uploaded to OneDrive yet: {}", pending.join(", "));
            }
            std::thread::sleep(remaining.min(Duration::from_millis(250)));
        }
        for (index, record) in records.iter().enumerate() {
            progress(index as f32 / records.len() as f32);
            let actual = hash::hash_file(&record.path)
                .with_context(|| format!("Could not download archive file: {}", record.relative))?;
            let size = fs::metadata(&record.path)
                .with_context(|| format!("Could not read archive size: {}", record.relative))?
                .len();
            if size != record.size || actual != record.hash {
                bail!("cloud verification failed: {}", record.relative);
            }
        }
        progress(1.0);
        Ok(CloudReport {
            files: records.len(),
            manifest: manifest
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        })
    })();
    let errors = guard.restore();
    if errors.is_empty() {
        result
    } else {
        let restore = format!("Could not restore pin states:\n{}", errors.join("\n"));
        match result {
            Ok(_) => bail!("{restore}"),
            Err(error) => bail!("{error:#}\n{restore}"),
        }
    }
}

#[cfg(windows)]
pub struct WindowsCloudFiles;

#[cfg(windows)]
impl WindowsCloudFiles {
    fn attributes(p: &Path) -> anyhow::Result<u32> {
        use windows::{Win32::Storage::FileSystem::GetFileAttributesW, core::HSTRING};
        let path = HSTRING::from(p.as_os_str());
        // SAFETY: path is a null-terminated UTF-16 string that outlives the call.
        let attributes = unsafe { GetFileAttributesW(&path) };
        if attributes == u32::MAX {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(attributes)
    }
}

#[cfg(windows)]
impl CloudFiles for WindowsCloudFiles {
    fn pin_state(&self, p: &Path) -> anyhow::Result<PinState> {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_UNPINNED};
        let attributes = Self::attributes(p)?;
        if attributes & FILE_ATTRIBUTE_PINNED.0 != 0 {
            Ok(PinState::Pinned)
        } else if attributes & FILE_ATTRIBUTE_UNPINNED.0 != 0 {
            Ok(PinState::Unpinned)
        } else {
            Ok(PinState::Neither)
        }
    }

    fn make_online_only(&self, p: &Path) -> anyhow::Result<()> {
        self.set_pin_state(p, PinState::Unpinned)
    }

    fn is_evicted(&self, p: &Path) -> anyhow::Result<bool> {
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
        };
        Ok(Self::attributes(p)?
            & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0 | FILE_ATTRIBUTE_OFFLINE.0)
            != 0)
    }

    fn set_pin_state(&self, p: &Path, s: PinState) -> anyhow::Result<()> {
        use windows::{
            Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
                FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_READONLY,
                FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_UNPINNED, FILE_FLAGS_AND_ATTRIBUTES,
                SetFileAttributesW,
            },
            core::HSTRING,
        };
        let keep = FILE_ATTRIBUTE_READONLY.0
            | FILE_ATTRIBUTE_HIDDEN.0
            | FILE_ATTRIBUTE_SYSTEM.0
            | FILE_ATTRIBUTE_ARCHIVE.0
            | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0;
        let pin = match s {
            PinState::Pinned => FILE_ATTRIBUTE_PINNED.0,
            PinState::Unpinned => FILE_ATTRIBUTE_UNPINNED.0,
            PinState::Neither => 0,
        };
        let attributes = match (Self::attributes(p)? & keep) | pin {
            // Windows expects NORMAL, not 0, for a file with no other attributes.
            0 => FILE_ATTRIBUTE_NORMAL,
            bits => FILE_FLAGS_AND_ATTRIBUTES(bits),
        };
        let path = HSTRING::from(p.as_os_str());
        // SAFETY: path outlives the call; only writable attributes and pin bits are passed.
        unsafe { SetFileAttributesW(&path, attributes) }?;
        Ok(())
    }
}
