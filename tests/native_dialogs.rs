#![cfg(all(feature = "desktop", target_os = "macos"))]
use std::{
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[test]
#[ignore = "opens real AppKit sheets; requires a macOS graphical session"]
fn native_open_and_export_sheets_cancel_without_crashing_or_losing_the_review() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("initial.sgf"), b"(;SZ[9];B[cc])").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_katastro-native-dialog-probe"))
        .arg(temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output().unwrap());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|error| {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            panic!("native dialog process did not finish: {error}");
        });
    let status = output.status;
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        status.success(),
        "native dialog process crashed: {status}\n{out}\n{err}"
    );
    assert!(
        out.contains("NATIVE_CANCEL_OK")
            && out.contains("NATIVE_REOPEN_CANCEL_OK")
            && out.contains("NATIVE_EXPORT_CANCEL_OK"),
        "{out}\n{err}"
    );
    let mut review = katastro::Review::new(
        &temp.path().join("reviews.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    review.import(&temp.path().join("initial.sgf")).unwrap();
    assert_eq!(review.document().unwrap().mainline.len(), 2);
    assert_eq!(
        std::fs::read(temp.path().join("initial.sgf")).unwrap(),
        b"(;SZ[9];B[cc])"
    );
}
