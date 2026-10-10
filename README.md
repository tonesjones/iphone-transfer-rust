# PhotoXfer

PhotoXfer saves photos and videos from your iCloud for Windows folder to your computer. It checks the saved files and creates a second copy in your local OneDrive folder. The app does not delete photos or verify that OneDrive has uploaded them.

## Back up your photos

If you have not built the app, follow [Set up the app](#set-up-the-app) first.

1. Let iCloud for Windows finish downloading the files you want to save.
2. Open **PhotoXfer Desktop.lnk** in the repository folder.
3. Select **Back Up Now**.
4. Keep the window open until the backup finishes.

The app saves new or missing files, checks the full saved library, and updates and checks the second copy. It stops if a step fails. Open **Problem details** to read the error, and keep your phone and iCloud copies until you resolve it.

**Both local copies passed their checks** means that the files on this computer match their recorded contents. Complete [the phone and cloud checks](#check-your-backups-before-deleting-photos) before deleting photos.

Expand **Your backup folders** to view the locations or open a folder. The defaults are:

| Folder | Location |
| --- | --- |
| Read from iCloud | `%USERPROFILE%\iCloudPhotos` |
| Saved library | `%USERPROFILE%\Pictures\iPhone Backup` |
| Second copy | `Photo Backups` in your OneDrive folder |

The source count can be lower than the saved count. Removing files from iCloud does not remove their saved copies. Files with capture dates go into year and month folders. Files without known dates stay in `_unsorted`.

## Check your backups before deleting photos

A successful local backup does not prove that every phone item was downloaded or that the cloud copy can be recovered. Complete both checks below before deleting any selected items from Apple Photos.

### Compare with your phone

Compare your phone's photos and videos with the saved library. File counts can differ from item counts because Live Photos contain a still photo and a motion video. Include edited versions you want to preserve.

For each Live Photo, confirm that both original components are saved. If either component is absent from the Windows folder, use an original export from Apple Photos and back up those files before deleting the phone copy.

### Verify a fresh cloud download

The second copy contains media files, database snapshots in `catalog-backups`, and matching recovery file lists in `manifests`. Each `.tsv` recovery file list records the contents expected in that backup batch.

1. Wait for OneDrive to report that the upload is complete.
2. Open the archive on the OneDrive website.
3. Download the latest recovery file list, every media file it lists, and its database snapshot into a separate folder outside OneDrive.
4. Keep the downloaded files at the same paths relative to that folder.
5. Verify the downloaded batch with the command below.

Replace the example restore folder and manifest filename with your actual download location and recovery file list:

```powershell
$restore = 'C:\PhotoXfer-Restore'
$manifest = Join-Path $restore 'manifests\library-TIMESTAMP.tsv'
.\target\release\photoxfer.exe verify --from $restore --manifest $manifest
```

Every listed media file and the database snapshot must pass. A copy made from the local OneDrive folder does not test cloud recovery. If verification fails, keep your phone and iCloud copies and resolve the reported problem.

The app has no deletion command. Do not disable the whole iCloud library or empty **Recently Deleted** as part of this workflow.

## Set up the app

Use Windows with Rust, Cargo, and the Windows build tools installed. Configure iCloud for Windows and OneDrive before your first backup.

Close any open PhotoXfer windows before rebuilding. In PowerShell, open the repository folder and build both programs:

```powershell
cargo build --release -p photoxfer-gui -p photoxfer-cli
```

Create the desktop shortcut:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-desktop-shortcut.ps1
```

The installer creates **PhotoXfer Desktop.lnk** in the repository folder. You can copy that shortcut to your desktop. It points to the compiled program in this repository, so recreate it if you move the repository.

The window uses Segoe UI fonts from Windows and falls back to bundled fonts if they are unavailable.

### Use different folders

Launch the desktop program with explicit paths:

```powershell
.\target\release\photoxfer-desktop.exe --from 'C:\Photo Inbox' --to 'C:\Saved Photos' --archive 'D:\Photo Backups'
```

These options apply to that launch. The saved library and second-copy folder must be separate, and neither can be inside the other.

## Use the console shortcut

For a text summary or troubleshooting, create the console shortcut:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-shortcut.ps1
```

Open **PhotoXfer Backup.lnk**. It runs the same backup steps in order, stops on failure, and waits for Enter before closing. **Backup finished successfully** confirms both local copies, subject to the same phone and cloud checks above.

To change folders when you run `scripts\backup.ps1` directly, pass `-Source`, `-Library`, or `-Archive`. Use `-Executable` to select a different command-line program. Use `-NoPause` for runs that must finish without waiting for Enter.

## Find a photo by its original filename

From the repository folder, run:

```powershell
.\target\release\photoxfer.exe find IMG_3353 --to "$env:USERPROFILE\Pictures\iPhone Backup"
```

The catalog retains source filenames encountered during imports. Nine files from the initial library have no recorded original names because they had left the source folder before name tracking began. Browse their date folders to find them. A matching original file can restore the missing name on a later import.

## Handle missing or damaged copies

Run the backup again to restore a missing saved copy if the matching source file is still available. Repeat imports check existing files before skipping them.

If a saved copy is damaged, the app preserves it and stops. Keep the damaged file separately, restore the saved copy from a verified source or archive, and run the checks again. There is no automatic repair command.

Imports save database snapshots in the library's `.catalog-backups` folder. The second copy also keeps snapshots and recovery file lists. These files accumulate because retention is not implemented.

Archive checks read every file's contents. OneDrive may download online-only files during each check. To avoid repeated downloads, select **Always keep on this device** for **Photo Backups** in OneDrive. Keeping the files locally uses computer disk space.
