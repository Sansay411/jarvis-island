use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=JARVIS_PAYLOAD");
    println!("cargo:rerun-if-env-changed=JARVIS_VERSION");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=installer.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.exe");
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
        embed_resource::compile("app.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
