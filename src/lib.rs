pub mod analysis;
pub mod archive;
#[cfg(feature = "desktop")]
pub mod desktop;
#[cfg(feature = "desktop")]
pub mod dialogs;
pub mod engine;
pub mod game;
// Retain the original public sync API for existing integrations.
pub use archive as pandanet;
pub mod review;
mod runtime;
pub mod scheduler;
pub mod training;
#[cfg(feature = "desktop")]
pub mod ui;
mod updates;
pub mod worker;
pub use analysis::{Analysis, EngineProfile, SuggestedMove};
pub use game::{Board, Color, Document, Move, Node, NodeId, NodePosition, Point};
pub use review::Review;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}
pub(crate) fn digest_file(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            return Ok(hex::encode(context.finish()));
        }
        context.update(&buffer[..size]);
    }
}
