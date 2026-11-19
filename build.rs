use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=runtime/stage.py");
    println!("cargo:rerun-if-changed=runtime/macos.json");
    println!("cargo:rerun-if-env-changed=KATASTRO_RUNTIME_CACHE");
    println!("cargo:rerun-if-env-changed=KATASTRO_OFFLINE");
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "macos"
        || env::var_os("CARGO_FEATURE_DESKTOP").is_none()
    {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let profile = out
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "build"))
        .unwrap()
        .parent()
        .unwrap();
    let manifest = fs::read_to_string("runtime/macos.json").unwrap();
    let lock: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    let name = format!("katastro-runtime-{}", lock["sha256"].as_str().unwrap());
    println!("cargo:rustc-env=KATASTRO_RUNTIME_NAME={name}");
    let ok = Command::new("/usr/bin/python3")
        .args(["runtime/stage.py"])
        .arg(profile)
        .status()
        .expect("run macOS runtime staging");
    assert!(ok.success(), "could not stage the pinned KataGo runtime");
    for file in [
        "Helpers/katago",
        "Resources/Models/review.bin.gz",
        "Resources/Models/b18c384nbt-humanv0.bin.gz",
        "Resources/Notices",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            profile.join(&name).join(file).display()
        );
    }
}
