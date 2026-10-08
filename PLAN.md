# photoxfer plan

A Windows app in Rust that copies photos and videos off an iPhone 11 Pro over USB, skips anything
already imported, and files everything into a date-based library. The full design (connection
choice, data model, iPhone gotchas, risks) is in the
[plan doc](https://claude.ai/code/artifact/2dec4039-e092-4cc2-905f-a121e69d9655). This file tracks
progress against it.

Last updated: 2026-10-08.

## Next checkpoint

Pick up here.

1. **You: run the WPD spike on the phone.** Follow [WINDOWS.md](WINDOWS.md). Send back
   `spike.log`, the `Get-FileHash` output, and any error text.
2. **You: merge PR #1** (squash) once the spike result is in. If the spike fails, fix it on the
   same branch first.
3. **Next session: phase 1 device import.** Start from the spike result. If WPD works, wire
   `crates/wpd` into the import (see phase 1's open items below). If WPD fails or is flaky,
   decide between fixing it and the fallbacks in the plan doc (Microsoft Photos into an inbox
   folder, or libimobiledevice over FFI).

Machine state: Rust 1.99 (MSVC) was installed with winget on 2026-10-08, plus the
`x86_64-pc-windows-gnu` target. MinGW is not installed, so the GNU-target check covers only
`crates/wpd`.

## Phase 0: WPD spike

Code done in PR #1. Waiting on the hardware run.

- [x] Enumerate portable devices and find "Apple iPhone".
  Accept: `wpd-spike` prints every device and picks the iPhone by friendly name or description.
- [x] List every file under `DCIM` with its size.
  Accept: prints `size<TAB>path` per file, then file count, folder count and total bytes.
- [x] Stream one file to disk.
  Accept: copies in chunks, never overwrites, exits 3 on a size mismatch.
- [ ] **Done when:** one HEIC copies off the phone, and its `Get-FileHash` matches a copy made
  through File Explorer (WINDOWS.md step 5).

## Phase 1: import CLI

- [x] `photoxfer import --from <folder> --to <library>` imports an inbox folder.
  Accept: recursive, skips symlinks, dot-files and a library nested in the inbox.
- [x] BLAKE3 hashing and the SQLite schema (assets, device_seen, live_pairs, sidecars, imports).
  Accept: `cargo test` covers schema, foreign keys and round trips.
- [x] Verify after copy.
  Accept: temp file, fsync, re-hash from disk, publish with `hard_link`; the DB row is written only
  after a match; a failed DB insert deletes the copy.
- [x] Live Photo pairing and AAE sidecars.
  Accept: HEIC, MOV and AAE share one stem and folder; `live_pairs` and `sidecars` rows exist.
- [x] A second run of the same inbox copies 0 files.
- [ ] Import straight from the phone (`photoxfer import --to <library>` with no `--from`).
  Accept: streams each DCIM file through the same verify-after-copy path; never reads a whole
  file into memory.
- [ ] Skip-if-seen with `device_seen`.
  Accept: a file whose (device, object id, size) is already recorded is skipped without reading
  its bytes.
- [ ] Resume after a lock or unplug.
  Accept: a dropped connection fails the run cleanly with the "keep the iPhone unlocked" hint, and
  the next run continues from `device_seen`.
- [ ] Show the count found on the phone next to the count imported (iCloud-optimized storage
  warning).
- [ ] **Done when:** a second run on the same phone copies 0 files in seconds.

## Phase 2: organizer

- [x] EXIF/QuickTime date extraction with `_unsorted` as the fallback (pulled into phase 1).
- [x] File into `YYYY/MM` with `capture-time_shorthash` names (pulled into phase 1).
- [ ] `photoxfer reindex` rebuilds the database from the library folder.
  Accept: deleting `library.db` and running reindex restores every asset row, and adopts library
  files the DB doesn't know about (see the crash gap below).
- [ ] Dedupe report across the whole library.
- [ ] **Done when:** the library is fully dated, has no duplicates, and the database rebuilds from
  disk.

## Phase 3: GUI and phase 4: Wi-Fi sync

Not started. See the plan doc.

## Open decisions

- **Orphan AAE files.** An AAE with no imported photo is currently stored as an asset of kind
  `sidecar`, because `sidecars.asset_hash` must reference an asset. Keep that, or change the
  schema?
- **Where device import lives.** Proposed: `crates/cli` depends on both `photoxfer-core` and
  `photoxfer-wpd`, and core gets a source-agnostic import entry point that takes a reader per file.
  This keeps core free of Windows code.
- **Default library location.** This sets the default `--to` path. Still open from the plan doc.
- **Deleting from the phone after import.** Not planned; WPD access is read-only.

## Known gaps

- A crash between publishing a library file and its DB insert leaves an unrecorded file. A rerun
  copies it again under a `-1` name. `reindex` (phase 2) is the planned cleanup.
- A hard kill can leave `.partial` files in the library.
- Directory entries aren't fsynced after publishing.
- Two photos with byte-identical AAE files: the AAE links to the first photo only.
