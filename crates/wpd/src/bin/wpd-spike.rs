#[cfg(not(windows))]
fn main() {
    eprintln!("wpd-spike only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        io::{BufWriter, Write},
        path::{Component, Path, PathBuf},
    };

    use anyhow::{Context, bail, ensure};
    use photoxfer_wpd::wpd::{Com, Device, find_iphone, list_devices};

    let mut out_dir = PathBuf::from(".");
    let mut requested_file = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out_dir = PathBuf::from(args.next().context("--out requires a directory")?),
            "--file" => requested_file = Some(args.next().context("--file requires a file name")?),
            _ => bail!("Unknown argument {arg:?}. Usage: wpd-spike [--out <dir>] [--file <name>]"),
        }
    }

    let com = Com::init()?;
    for info in list_devices(&com)? {
        println!("{}\t{}\t{}", info.friendly_name, info.manufacturer, info.id);
    }
    let Some(info) = find_iphone(&com)? else {
        eprintln!(
            "No Apple iPhone found. Install the Apple Devices app, keep the iPhone unlocked and tap Trust."
        );
        drop(com);
        std::process::exit(2);
    };
    let device = Device::open(&com, &info.id)?;
    let entries = device.list_dcim()?;
    let mut file_count = 0;
    let mut folder_count = 0;
    let mut total_bytes = 0_u64;
    for entry in &entries {
        if entry.is_folder {
            folder_count += 1;
        } else {
            println!("{}\t{}", entry.size, entry.path);
            file_count += 1;
            total_bytes = total_bytes
                .checked_add(entry.size)
                .context("listed total bytes overflow")?;
        }
    }
    println!("Files: {file_count}\tFolders: {folder_count}\tTotal bytes: {total_bytes}");

    let selected = if let Some(name) = &requested_file {
        Some(
            entries
                .iter()
                .find(|entry| !entry.is_folder && entry.name.eq_ignore_ascii_case(name))
                .with_context(|| format!("Requested file {name:?} was not found in DCIM"))?,
        )
    } else {
        entries
            .iter()
            .find(|entry| !entry.is_folder && entry.name.to_ascii_lowercase().ends_with(".heic"))
            .or_else(|| entries.iter().find(|entry| !entry.is_folder))
    };
    let Some(entry) = selected else {
        println!("DCIM has no files to copy; keep the iPhone unlocked and tap Trust.");
        return Ok(());
    };

    // Treat device names as a single file name, never as an output path.
    let mut components = Path::new(&entry.name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && !entry.name.contains(['/', '\\', ':']),
        "Device returned an unsafe output file name: {:?}",
        entry.name
    );
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create output directory {}", out_dir.display()))?;
    let destination = out_dir.join(&entry.name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .with_context(|| {
            format!(
                "create {} (existing files are never overwritten)",
                destination.display()
            )
        })?;
    let mut out = BufWriter::new(file);
    let written = device.read_to(&entry.object_id, &mut out)?;
    out.flush()
        .with_context(|| format!("flush {}", destination.display()))?;
    println!(
        "Copied {} bytes to {}; listed size: {}",
        written,
        destination.display(),
        entry.size
    );
    if written != entry.size {
        eprintln!(
            "Size mismatch: copied {written} bytes, expected {}.",
            entry.size
        );
        // Release the file, device and COM guard before process::exit skips destructors.
        drop(out);
        drop(device);
        drop(com);
        std::process::exit(3);
    }
    Ok(())
}
