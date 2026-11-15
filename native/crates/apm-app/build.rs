//! Windows executable resources: the app icon and the version information Explorer, Task Manager
//! and the installer show (plan §6.5). The application manifest (PerMonitorV2 DPI awareness,
//! Common Controls v6, asInvoker, Windows 10 compatibility, segment heap) comes from GPUI's
//! `windows-manifest` feature, which gpui-platform always turns on; a second RT_MANIFEST here would
//! not link, so this file adds only the icon and VERSIONINFO.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest_dir.join("../../../resources/icon.ico").canonicalize().expect("resources/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    // FILEVERSION is four 16-bit numbers: the numeric core, and 0 (a pre-release has no number
    // that sorts correctly here; the strings carry the full version).
    let core = version.split(['-', '+']).next().unwrap_or("0.0.0");
    let mut nums: Vec<u16> = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
    nums.resize(4, 0);
    let numeric = nums.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let year = 2026;

    // Same product strings as 1.x (electron-builder: productName, "Copyright © <year> <productName>").
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "AWS Profile Manager"
      VALUE "FileDescription", "AWS Profile Manager"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "AWS Profile Manager"
      VALUE "LegalCopyright", "Copyright © {year} AWS Profile Manager"
      VALUE "OriginalFilename", "AWS Profile Manager.exe"
      VALUE "ProductName", "AWS Profile Manager"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/"),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
