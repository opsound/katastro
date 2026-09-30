#![cfg(all(feature = "desktop", target_os = "macos"))]
use std::process::Command;

#[test]
fn packaging_produces_a_signed_macos_app_with_a_launchable_executable() {
    let temp = tempfile::TempDir::new().unwrap();
    let bundle = temp.path().join("Katastro.app");
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/package-macos.sh"
        ))
        .arg(&bundle)
        .env("KATASTRO_BINARY", env!("CARGO_BIN_EXE_katastro"))
        .status()
        .unwrap();
    assert!(status.success());
    let plist = bundle.join("Contents/Info.plist");
    assert!(
        plist.is_file(),
        "a launchable app needs its bundle metadata"
    );
    let name = Command::new("plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .unwrap();
    assert!(name.status.success());
    let binary = bundle
        .join("Contents/MacOS")
        .join(String::from_utf8(name.stdout).unwrap().trim());
    let kind = Command::new("file").arg(&binary).output().unwrap();
    assert!(String::from_utf8(kind.stdout).unwrap().contains("Mach-O"));
    assert!(
        Command::new("test")
            .arg("-x")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(&bundle)
            .status()
            .unwrap()
            .success()
    );
}
