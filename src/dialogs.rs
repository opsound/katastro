use eframe::Frame;
use std::{future::Future, path::PathBuf, pin::Pin};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogRequest {
    OpenGame,
    ExportGame { filename: String },
    EngineExecutable,
    EngineModel,
}
pub type DialogFuture = Pin<Box<dyn Future<Output = Option<PathBuf>>>>;

pub trait FileDialogs {
    fn start(&mut self, request: &DialogRequest, parent: &Frame) -> DialogFuture;
}

pub struct NativeDialogs;
impl FileDialogs for NativeDialogs {
    fn start(&mut self, request: &DialogRequest, parent: &Frame) -> DialogFuture {
        // A parented async dialog becomes an AppKit sheet. A synchronous panel's
        // runModal would reenter winit while it is handling the current UI event.
        let dialog = rfd::AsyncFileDialog::new().set_parent(parent);
        let (dialog, save) = match request {
            DialogRequest::OpenGame => (dialog.add_filter("Go game", &["sgf"]), false),
            DialogRequest::ExportGame { filename } => (
                dialog
                    .add_filter("Go game", &["sgf"])
                    .set_file_name(filename),
                true,
            ),
            DialogRequest::EngineExecutable => (dialog, false),
            DialogRequest::EngineModel => (dialog.add_filter("KataGo model", &["gz"]), false),
        };
        Box::pin(async move {
            let file = if save {
                dialog.save_file().await
            } else {
                dialog.pick_file().await
            };
            file.map(|file| file.path().to_path_buf())
        })
    }
}
