pub mod analysis;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod engine;
pub mod game;
pub mod review;
pub mod scheduler;
#[cfg(feature = "desktop")]
pub mod ui;
pub mod worker;
pub use analysis::{Analysis, EngineProfile};
pub use game::{Board, Color, Document, Move, Node, NodeId, NodePosition, Point};
pub use review::Review;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
