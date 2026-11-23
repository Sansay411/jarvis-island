use std::path::{Path, PathBuf};

/// Version resource and manifest for one of the two programs. A complete
/// VERSIONINFO and an `asInvoker` manifest matter beyond looks: Windows
/// elevates unmanifested files named *setup*/*uninstall* on its own, and
/// antiviruses trust files that describe themselves.
fn resources(out: &Path, bin: &str, id: &str, description: &str, version: &str) {
    let core = version.split(['-', '+']).next().unwrap_or("0.0.0");
    let mut nums: Vec<&str> = core.split('.').collect();
    nums.resize(4, "0");
    let manifest = std::fs::read_to_string("app.manifest")
        .unwrap()
        .replace("@NAME@", id)
        .replace("@VERSION@", &nums.join("."));
    let manifest_path = out.join(format!("{bin}.manifest"));
    std::fs::write(&manifest_path, manifest).unwrap();
    let icon = std::fs::canonicalize("installer.ico").unwrap();
    let esc = |p: &Path| p.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "\\\\");
    let rc = format!(
        r#"#pragma code_page(65001)
#include <winver.h>

1 ICON "{icon}"
1 24 "{manifest}"

VS_VERSION_INFO VERSIONINFO
FILEVERSION {nums}
PRODUCTVERSION {nums}
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041904B0"
    BEGIN
      VALUE "CompanyName", "Jarvis"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{bin}"
      VALUE "LegalCopyright", "© 2026 Sansay411, MIT License"
      VALUE "OriginalFilename", "{bin}.exe"
      VALUE "ProductName", "Jarvis"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x419, 1200
  END
END
"#,
        icon = esc(&icon),
        manifest = esc(&manifest_path),
        nums = nums.join(","),
    );
    let rc_path = out.join(format!("{bin}.rc"));
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile_for(&rc_path, [bin], embed_resource::NONE).manifest_required().unwrap();
}

fn main() {
    println!("cargo:rerun-if-env-changed=JARVIS_PAYLOAD");
    println!("cargo:rerun-if-env-changed=JARVIS_VERSION");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=installer.ico");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out = out_dir.join("payload.exe");
    match std::env::var("JARVIS_PAYLOAD") {
        Ok(path) if !path.trim().is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(&path, &out).unwrap_or_else(|e| panic!("JARVIS_PAYLOAD {path}: {e}"));
        }
        _ => std::fs::write(&out, []).unwrap(),
    }
    let version = std::env::var("JARVIS_VERSION").unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=JARVIS_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        resources(&out_dir, "jarvis-setup", "Jarvis.Setup", "Установка Джарвиса", &version);
        resources(&out_dir, "jarvis-uninstall", "Jarvis.Uninstall", "Удаление Джарвиса", &version);
    }
}
