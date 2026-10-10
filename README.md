# PhotoXfer

## Desktop backup window

Build and create the desktop-app shortcut:

```powershell
cargo build --release -p photoxfer-gui
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-desktop-shortcut.ps1
```

Double-click **PhotoXfer Desktop.lnk**, then select **Back Up Now**. The window saves new files, checks the full saved library, and updates and verifies **Photo Backups**. It stays responsive during work and stops at the first failed step. Keep it open until the run finishes. Folder buttons open your source, saved library, and second copy.

**Both local copies passed their checks** confirms files on this computer. The separate **Before deleting photos** checklist remains unverified until you complete the phone comparison and fresh cloud-download check described below. The app does not delete files or confirm OneDrive upload.

Defaults follow your Windows user folder and OneDrive location. For a different setup, launch `photoxfer-desktop.exe` with `--from`, `--to`, and `--archive` paths. The console shortcut below remains available for troubleshooting.

## Windows backup

Build the release executable from the repository folder:

```powershell
cargo build --release -p photoxfer-cli
```

Install the launcher shortcut in the repository folder:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-shortcut.ps1
```

Double-click **PhotoXfer Backup.lnk** to import from `C:\Users\Owner\iCloudPhotos`, check the import, and archive the library to `C:\Users\Owner\OneDrive\Photo Backups`. The launcher runs each step in order and stops if one fails. When running `scripts\backup.ps1` directly, override paths with `-Source`, `-Library`, `-Archive`, or `-Executable`; use `-NoPause` for automated runs.

The window shows three steps: save photos and videos, check your saved library, and update and check your second copy. **Backup finished successfully** means both copies on this computer passed their checks. The source count can be lower than the saved count because removing a file from iCloud does not remove its backup. The final checklist shows what still needs checking before deleting photos from your phone or iCloud.

Compare the phone's total photo and video count with the imported library, including Live Photos and edited versions. Keep the source files until a full manifest batch has been restored from the cloud archive and checked. Cloud upload is not verified by this launcher, so manual cleanup remains blocked until that restore check succeeds. Items with unknown dates stay unsorted.

## Find an original filename

```powershell
.\target\release\photoxfer.exe find IMG_3353 --to C:\Users\Owner\Pictures\photoxfer
```

The catalog retains every source name encountered. Old names can only be recovered when a matching source file is still available. Repeat imports verify existing copies, restore missing copies, and stop on damaged copies without overwriting them. Catalog snapshots are saved in the library's `.catalog-backups` folder.

Nine files from the initial library have no known original names because they had left the source before name tracking began. Browse their date folders to find them. A damaged working copy currently needs manual restoration from a verified source or archive; keep the damaged file separately and check the replacement before continuing.

## Verify a cloud restore before cleanup

The archive keeps ordinary media copies, dated SQLite snapshots under `catalog-backups`, and matching manifests under `manifests`. It never mirrors deletions. A successful archive run verifies local copies only.

Verification reads full file contents, so online-only OneDrive files can be downloaded again. Keeping the archive available locally avoids that repeated download. Snapshot retention is not implemented yet; snapshots and manifests accumulate.

1. Confirm OneDrive reports the upload complete and inspect the archive on the OneDrive website.
2. Download the media listed in the latest manifest, its database snapshot, and that manifest from the website into a separate folder outside OneDrive. Preserve their paths relative to the archive root. This must be a fresh cloud download, not a local folder copy.
3. Verify that downloaded batch:

```powershell
.\target\release\photoxfer.exe verify --from C:\PhotoXfer-Restore --manifest C:\PhotoXfer-Restore\manifests\library-TIMESTAMP.tsv
```

Replace the example paths with your download location and actual manifest filename. Every listed media file and the database snapshot must match. A second folder on the same PC only proves cloud recovery if its files were downloaded from the cloud.

Before deleting any selected items in Apple Photos, confirm coverage on the phone. File counts can differ from photo counts. Check the known Live Photo: preserve and verify both its original still and motion component using an Apple-supported original export if the Windows folder lacks either. There is no deletion command. Do not disable the whole iCloud library or empty Recently Deleted as part of this workflow.
