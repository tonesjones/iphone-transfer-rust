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

## Next checkpoint: needed now

Use `PhotoXfer Backup.lnk` for import, check, then archive. Keep the working library outside OneDrive; the separate archive is `C:\Users\Owner\OneDrive\PhotoXfer Archive`. A local archive result does not prove cloud upload.

- [ ] Phone completeness: user reports 33 videos, 5 selfies, 1 Live Photo, 14 screenshots, and no known edited photos. Total items remain unknown. Counts overlap. Verify both original components of the one Live Photo; use an original export if either is absent.
- [ ] Cloud recovery: confirm upload, then download every file listed in the latest manifest plus the manifest from OneDrive to a separate folder outside OneDrive. Run `photoxfer verify --from <restore> --manifest <manifest>`. Accept: every listed media file and catalog snapshot matches.
- [ ] Manual cleanup: selected items only, after phone completeness and cloud recovery checks. No app deletion, whole-library iCloud shutdown, or emptying Recently Deleted.

Phase 1 reliability and the second archive are needed now. Phase 2 recovery/dating and phase 3 GUI are nice later. No new packages: the existing SQLite backup feature is enabled.

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
- [x] `photoxfer check --from <source> --to <library>`: read-only. Re-hashes every library file
  recorded in `library.db`, then confirms each source file has a verified copy. Reports missing
  source files, damaged or deleted library copies, and untracked library files; exits 1 if any.
  Real run 2026-10-10: 110 safely in library, 0 problems, 0.6 s.
- [x] Repeat imports verify saved bytes before skipping, restore missing copies, and report damaged copies without overwriting. Accept: missing/corrupt-copy regression tests pass.
- [x] `check` opens SQLite read-only without migrations and includes library scan errors. Accept: a read-only legacy catalog is unchanged; inaccessible scans produce errors.
- [x] Source names and paths are retained, including aliases for identical bytes. `photoxfer find <text> --to <library>` searches them. Accept: IMG_3353 is searchable after import.
- [x] Consistent SQLite snapshots before migration and after successful imports. Accept: recovered snapshots pass integrity checks and retain source-name aliases.
- [x] `photoxfer archive --from <library> --to <archive>` creates independent verified copies, a SQLite snapshot, and a manifest. `verify --from <restore> --manifest <manifest>` checks recovery. Accept: repeat copies zero media; conflicts are preserved; corruption fails; working-file edits do not affect archive bytes.
- [x] Shortcut runs import, check, archive, stops on failure and waits for Enter. Accept: success/failure scenarios and spaced paths work.
- [x] Real updated run: import IMG_3353.PNG and verify all current source files. Accept: no missing/damaged/unreadable files and existing media bytes unchanged; counts measured at runtime.

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
- [x] File into `YYYY/<Month name>` (e.g. `2026/October`, changed from `YYYY/MM` on 2026-10-10) with `capture-time_shorthash` names (pulled into phase 1).
- [ ] `photoxfer reindex` rebuilds the database from the library folder.
  Accept: deleting `library.db` and running reindex restores every asset row, and adopts library
  files the DB doesn't know about (see the crash gap below).
- [ ] Dedupe report across the whole library.
- [ ] Date suggestions from original filenames, then source timestamps explicitly labeled as estimates; preview moves, manually date only what remains, and keep unknown dates unknown.
- [ ] **Done when:** known dates are organized, uncertainty is visible, and the file catalog rebuilds from disk. Source names and relationships absent from media require the SQLite snapshots.

## Phase 3: GUI and phase 4: Wi-Fi sync

Deferred until the shortcut exposes a concrete usability limitation. Review dependencies and security implications before selecting a GUI toolkit.

## Updated run evidence (2026-10-10)

- Imported IMG_3353.PNG: copied 1, verified/skipped 101. Current library has 111 assets; 31 are undated.
- Check: 102 current source files verified, zero missing, damaged, untracked, or unreadable.
- All 110 pre-existing media files retain their SHA-256 hashes.
- OneDrive local archive: 111 media files verified; snapshot integrity and full manifest verification passed.
- Latest manifest: `manifests/library-20261010T203720.564827500Z.tsv`.
- Validation: 56 tests passed, compiler checks clean; truncated manifests fail verification.
- Cloud upload/restore remains unverified: in-app browser requires OneDrive sign-in. Phone total and both original components of the one Live Photo remain to be confirmed.
- Sol owned implementation and final review; the launcher was delegated to a runtime-verified Luna session using tokenomics. Savings are unknown.

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

- Nine of the initial 111 assets have no original-name record because their source files were already absent when source-name tracking began. `find` cannot locate these by their old names; they remain accessible through their library paths. A matching original source or older export could recover those names.
- A damaged working copy is preserved and reported, but has no automatic repair command. Follow-up: restore from a verified source or archive, preserve the damaged file separately, and verify the replacement before resuming backup.
- Archive verification reads every saved file. OneDrive may download online-only files during these checks. Keep full hash verification; keeping the archive available locally avoids repeated downloads at the cost of disk space.
- Catalog snapshots and archive manifests accumulate. Follow-up: retain the last 10 completed recovery sets, keeping each archive snapshot and its manifest together; never delete media as part of retention.
- Source paths currently retain Windows' extended-path prefix. This affects presentation only; a later display cleanup can omit it without changing stored paths.
- A crash between publishing a library file and its DB insert leaves an unrecorded file. A rerun
  copies it again under a `-1` name. `reindex` (phase 2) is the planned cleanup.
- A hard kill can leave `.partial` files in the library.
- Directory entries aren't fsynced after publishing.
- Two photos with byte-identical AAE files: the AAE links to the first photo only.
