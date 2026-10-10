# photoxfer plan

A Windows app in Rust that copies photos and videos off an iPhone 11 Pro over USB, skips anything
already imported, and files everything into a date-based library. The full design (connection
choice, data model, iPhone gotchas, risks) is in the
[plan doc](https://claude.ai/code/artifact/2dec4039-e092-4cc2-905f-a121e69d9655). This file tracks
progress against it.

Last updated: 2026-10-10.

## Decision 2026-10-10: import from iCloud, park USB

USB/WPD access is blocked on the phone side. The spike finds "Apple iPhone", but `Internal Storage`
stays empty, in File Explorer too, even after installing Apple Devices and tapping Trust (one run
got `GetValues(s10003)` → `0x80042008`). iCloud for Windows already mirrors the library to
`C:\Users\Owner\iCloudPhotos`, so the source is now that folder:

```powershell
photoxfer import --from C:\Users\Owner\iCloudPhotos --to C:\Users\Owner\Pictures\photoxfer
```

Done on 2026-10-10: all 110 iCloud files imported, a re-run copied 0, and a SHA-256 cross-check
found every source file in the library with nothing missing or extra. The iCloud folder is pinned
("Always keep on this device") so originals stay downloaded.

The library is outside OneDrive on purpose: syncing `library.db` mid-write risks corruption, and
OneDrive handles hard links poorly.

## Next checkpoint

Pick up here.

1. **You: confirm completeness.** Compare the Photos app count (Library → All Photos, bottom of
   the list) with the iCloud folder's 110 files. Filenames reach IMG_3352, so the phone may hold
   more; if so, check iCloud Photos is on for the phone and in iCloud for Windows.
2. **You: spot-check the library** in `C:\Users\Owner\Pictures\photoxfer`, then delete
   `C:\Users\Owner\Pictures\photoxfer-run1` (an earlier run that also copied two `desktop.ini`
   files).
3. **Later: free iCloud space.** Only after the library has proven itself over time, and with a
   backup of it. Deleting from iCloud also deletes from the phone, so the order is: turn off
   iCloud Photos on the phone (keeping local copies), then Manage Storage → Photos → Turn Off &
   Delete. You do this step; it isn't automated.

Machine state: Rust 1.99 (MSVC) and the `x86_64-pc-windows-gnu` target (winget, 2026-10-08);
Apple Devices 1.1540 (winget, 2026-10-10). MinGW is not installed, so the GNU-target check covers
only `crates/wpd`.

## Phase 0: WPD spike

Code done in PR #1. **Parked:** the hardware run is blocked on the phone side (see the decision
above). Retry only once File Explorer shows `Internal Storage\DCIM`.

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
- [x] Non-media files (`desktop.ini` and similar) are ignored and counted, not copied.
- [x] Quiet output: one summary line; `nom_exif` logging is off and other logs go to stderr.
- [x] Real run on the iCloud folder (110 files, re-run copies 0, SHA-256 cross-check clean).

Parked with phase 0 (only needed if USB comes back):

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
- **Default library location.** Decided 2026-10-10: `C:\Users\Owner\Pictures\photoxfer`. `--to`
  is still required on the command line.
- **Deleting from the phone or iCloud after import.** Wanted eventually, once the library has
  been validated over time. Not automated: the iCloud folder is a two-way sync, so deleting there
  deletes from the phone too. Revisit with a backup in place.

## Known gaps

- A crash between publishing a library file and its DB insert leaves an unrecorded file. A rerun
  copies it again under a `-1` name. `reindex` (phase 2) is the planned cleanup.
- A hard kill can leave `.partial` files in the library.
- Directory entries aren't fsynced after publishing.
- Two photos with byte-identical AAE files: the AAE links to the first photo only.
