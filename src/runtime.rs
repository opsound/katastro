//! Resolve immutable resources relative to the running binary, including after relocation.
use std::path::PathBuf;

pub(crate) fn directory() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    if bin.ends_with("Contents/MacOS") {
        return Some(bin.parent()?.to_owned());
    }
    let name = option_env!("KATASTRO_RUNTIME_NAME")?;
    // Cargo integration-test binaries may live below the profile directory.
    bin.ancestors().map(|p| p.join(name)).find(|p| p.is_dir())
}

pub(crate) fn human_model() -> Option<PathBuf> {
    Some(
        directory()?
            .join("Resources/Models")
            .join(crate::training::client::HUMAN_MODEL),
    )
}
