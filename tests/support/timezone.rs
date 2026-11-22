// TZ is process-wide. Run this one test in a child when it needs a specific zone.
pub fn rerun_if_needed(test: &str, zone: &str) -> bool {
    if std::env::var("TZ").as_deref() == Ok(zone) {
        return false;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("TZ", zone)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{test} failed in {zone}\n{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    true
}
