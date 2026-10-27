//! Windows only: embed the app icon, version info (VERSIONINFO) and the application manifest
//! (per-monitor DPI awareness: `src/windows_manifest.rs`) into `downloadcraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `DOWNLOADCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (release builds: packaging/windows/package.ps1).

#[path = "src/windows_manifest.rs"]
mod windows_manifest;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/windows_manifest.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/downloadcraft.ico");
    println!("cargo:rerun-if-env-changed=DOWNLOADCRAFT_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=DOWNLOADCRAFT_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=DOWNLOADCRAFT_BUILD_DATE");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/downloadcraft.ico")
        .set_manifest(windows_manifest::WINDOWS_MANIFEST)
        .set("ProductName", "DownloadCraft")
        .set("FileDescription", "DownloadCraft download manager")
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) the DownloadCraft authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "downloadcraft.exe")
        .set("InternalName", "downloadcraft");
    if let Err(e) = res.compile() {
        if std::env::var_os("DOWNLOADCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=downloadcraft.exe built without icon/version resources: {e}");
    }
}
