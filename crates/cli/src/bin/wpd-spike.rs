#[cfg(not(windows))]
fn main() {
    eprintln!("wpd-spike only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    todo!()
}
