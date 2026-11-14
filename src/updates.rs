//! A snapshot is queued before requesting its UI repaint.
use std::sync::{
    Arc,
    mpsc::{self, Receiver},
};
pub(crate) type Wake = Option<Arc<dyn Fn() + Send + Sync>>;
pub(crate) fn channel<T: Send>(wake: Wake) -> (impl Fn(T) + Send, Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    (
        move |state| {
            let _ = tx.send(state);
            if let Some(wake) = &wake {
                wake();
            }
        },
        rx,
    )
}
