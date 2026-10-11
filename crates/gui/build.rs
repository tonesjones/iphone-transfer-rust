fn main() {
    println!("cargo:rerun-if-changed=icons/photoxfer.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("icons/photoxfer.ico")
            .compile()
            .expect("embed the app icon");
    }
}
