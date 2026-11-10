//! On Windows, puts the icon and the name and version into copperview.exe, so
//! Explorer, the taskbar and Task Manager show Copperview rather than a blank.
//! Other systems need nothing here.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows/copperview.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/windows/copperview.ico")
            .set("ProductName", "Copperview")
            .set("FileDescription", "Copperview")
            .set("LegalCopyright", "MIT License");
        if let Err(error) = res.compile() {
            // A missing resource compiler costs the icon, not the build.
            println!("cargo:warning=The Windows icon couldn't be added: {error}");
        }
    }
}
