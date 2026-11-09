//! On Windows, embeds the app icon and version details into the executable, so Explorer,
//! the taskbar and the installer show them.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/recvrve.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/recvrve.ico")
            .set("ProductName", "Recvrve")
            .set("FileDescription", &format!("Recvrve {version} Beta"))
            .set("ProductVersion", &format!("{version} Beta"))
            .set(
                "LegalCopyright",
                "Copyright (C) 2026 odparovacc. GNU GPL v2 or later.",
            );
        res.compile()
            .expect("could not embed the Windows icon and version details");
    }
}
