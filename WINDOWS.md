# Running the WPD spike on Windows

This is phase 0: prove that Rust can see the iPhone over USB, list `DCIM`, and copy one file off it
with matching bytes. It takes about 20 minutes the first time, most of it installing tools.

## 1. One-time setup

1. **Visual Studio Build Tools** (the C/C++ linker Rust needs, and the compiler for the bundled SQLite).
   Download "Build Tools for Visual Studio" from <https://visualstudio.microsoft.com/downloads/>,
   run it, tick **Desktop development with C++**, and install.
2. **Rust**. Download and run `rustup-init.exe` from <https://rustup.rs>. Accept the defaults
   (toolchain `stable-x86_64-pc-windows-msvc`). Open a new PowerShell window and check:
   ```powershell
   cargo --version
   ```
3. **Git**, if you don't have it: <https://git-scm.com/download/win>.
4. **Apple Devices** from the Microsoft Store (or iTunes). This installs the USB driver; without it
   the phone never shows up as a portable device.
5. On the iPhone: **Settings → Photos → Transfer to Mac or PC → Keep Originals**. Otherwise Windows
   gets JPEG conversions instead of the original HEIC files, and the sizes won't match the library.

## 2. Get the code

```powershell
cd $HOME
git clone https://github.com/tonesjones/iphone-transfer-rust.git
cd iphone-transfer-rust
git checkout claude/photoxfer-phase0-1
```

Build and run the tests first (this also proves SQLite compiles with MSVC):

```powershell
cargo test --workspace
```

All tests should pass.

## 3. Connect the phone

1. Plug the iPhone into the PC with a USB cable and **unlock it**.
2. If the phone asks **Trust This Computer?**, tap **Trust** and enter your passcode.
3. Open File Explorer → **This PC**. You should see **Apple iPhone**. Open it and confirm you can see
   `Internal Storage\DCIM`. If Explorer can't see it, the spike can't either; fix that first
   (re-plug, unlock, re-trust, or reinstall Apple Devices).
4. Keep the phone unlocked for the whole run. Set **Settings → Display & Brightness → Auto-Lock** to
   **Never** for now if it keeps locking.

## 4. Run the spike

```powershell
mkdir C:\photoxfer-spike
cargo run --release -p photoxfer-wpd --bin wpd-spike -- --out C:\photoxfer-spike > C:\photoxfer-spike\spike.log
type C:\photoxfer-spike\spike.log | Select-Object -Last 15
```

The spike:

1. lists every portable device it finds,
2. picks the one named "Apple iPhone",
3. prints every file under `DCIM` with its size, then totals,
4. copies the first `.HEIC` file to `C:\photoxfer-spike\` and checks the byte count.

To copy a specific file instead, add `--file IMG_1234.HEIC`.

Exit codes: `0` success, `2` no iPhone found, `3` copied size doesn't match the listed size,
`1` any other error (the message says which WPD call failed).

## 5. Check the bytes match (the phase 0 "done when")

Copy the **same** file a second way, through File Explorer, so there's an independent reference:

1. In Explorer, open `Apple iPhone\Internal Storage\DCIM\<folder>` and find the file the spike
   copied (its path is in `spike.log`).
2. Copy it to `C:\photoxfer-spike\explorer\`.
3. Compare hashes:
   ```powershell
   Get-FileHash C:\photoxfer-spike\IMG_XXXX.HEIC, C:\photoxfer-spike\explorer\IMG_XXXX.HEIC
   ```
   The two `Hash` values must be identical.

Also compare the file count the spike printed with the count in the Photos app. A lower spike count
usually means **Optimize iPhone Storage** is on and some originals are only in iCloud.

## 6. Send back

- `C:\photoxfer-spike\spike.log` (it contains file names but no photo content),
- the `Get-FileHash` output,
- any error text if a step failed.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| `no iPhone found` (exit 2) | Phone locked, Trust not tapped, or Apple Devices not installed. |
| Device found, `DCIM` empty or access denied | Phone locked during the run. Unlock and re-run. |
| `link.exe not found` during `cargo` | Build Tools missing the C++ workload (step 1.1). |
| Spike copies a `.JPG` where you expected `.HEIC` | "Keep Originals" is not set (step 1.5). |
