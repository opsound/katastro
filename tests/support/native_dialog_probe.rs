// A subprocess is required: AppKit/winit must run on the process's main thread,
// and a reentrant event-loop panic aborts rather than unwinding into a test.
#[cfg(target_os = "macos")]
mod probe {
    use eframe::{App, egui};
    use katastro::{desktop::DesktopApp, engine::EngineConfig};
    use objc2::{class, msg_send, runtime::AnyObject};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    // These calls only touch this probe process's own AppKit windows. They need
    // no system-wide accessibility permissions or desktop input automation.
    unsafe fn sheet() -> *mut AnyObject {
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let windows: *mut AnyObject = msg_send![app, windows];
            let count: usize = msg_send![windows, count];
            for index in 0..count {
                let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
                let sheet: *mut AnyObject = msg_send![window, attachedSheet];
                if !sheet.is_null() {
                    return sheet;
                }
            }
            std::ptr::null_mut()
        }
    }
    fn shortcut(ui: &mut egui::Ui, key: egui::Key) {
        ui.input_mut(|input| {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    mac_cmd: true,
                    ..Default::default()
                },
            })
        });
    }
    struct Probe {
        app: DesktopApp,
        initial: PathBuf,
        phase: u8,
        deadline: Instant,
    }
    impl App for Probe {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            assert!(
                Instant::now() < self.deadline,
                "native dialog probe timed out in phase {}",
                self.phase
            );
            match self.phase {
                0 if self.app.state.source.as_ref() == Some(&self.initial) => {
                    shortcut(ui, egui::Key::O);
                    self.phase = 1;
                }
                3 => {
                    shortcut(ui, egui::Key::O);
                    self.phase = 4;
                }
                6 => {
                    shortcut(ui, egui::Key::S);
                    self.phase = 7;
                }
                _ => {}
            }
            self.app.ui(ui, frame);
            assert!(self.app.state.error.is_none(), "{:?}", self.app.state.error);
            let panel = unsafe { sheet() };
            match self.phase {
                1 if !panel.is_null() => {
                    unsafe {
                        let _: () = msg_send![panel, cancel: std::ptr::null::<AnyObject>()];
                    }
                    self.phase = 2;
                }
                2 if panel.is_null() => {
                    assert_eq!(self.app.state.source.as_ref(), Some(&self.initial));
                    println!("NATIVE_CANCEL_OK");
                    self.phase = 3;
                }
                4 if !panel.is_null() => {
                    unsafe {
                        let _: () = msg_send![panel, cancel: std::ptr::null::<AnyObject>()];
                    }
                    self.phase = 5;
                }
                5 if panel.is_null() => {
                    assert_eq!(self.app.state.source.as_ref(), Some(&self.initial));
                    assert_eq!(self.app.state.document.as_ref().unwrap().mainline.len(), 2);
                    println!("NATIVE_REOPEN_CANCEL_OK");
                    self.phase = 6;
                }
                7 if !panel.is_null() => {
                    unsafe {
                        let _: () = msg_send![panel, cancel: std::ptr::null::<AnyObject>()];
                    }
                    self.phase = 8;
                }
                8 if panel.is_null() => {
                    assert_eq!(self.app.state.source.as_ref(), Some(&self.initial));
                    assert_eq!(self.app.state.document.as_ref().unwrap().mainline.len(), 2);
                    assert!(!self.app.state.status.starts_with("Exported "));
                    println!("NATIVE_EXPORT_CANCEL_OK");
                    self.phase = 9;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }
    pub fn run() -> eframe::Result {
        let root = std::fs::canonicalize(PathBuf::from(
            std::env::args_os()
                .nth(1)
                .expect("temporary fixture directory"),
        ))
        .unwrap();
        let initial = root.join("initial.sgf");
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Katastro native dialog regression")
                .with_inner_size([1200.0, 860.0]),
            ..Default::default()
        };
        eframe::run_native(
            "Katastro native dialog regression",
            options,
            Box::new(move |cc| {
                let app = DesktopApp::new(
                    &cc.egui_ctx,
                    root.join("reviews.sqlite"),
                    root.join("cache.sqlite"),
                    EngineConfig {
                        executable: "missing".into(),
                        model: "missing".into(),
                    },
                    Some(initial.clone()),
                );
                Ok(Box::new(Probe {
                    app,
                    initial,
                    phase: 0,
                    deadline: Instant::now() + Duration::from_secs(20),
                }))
            }),
        )
    }
}
#[cfg(target_os = "macos")]
fn main() -> eframe::Result {
    probe::run()
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("native dialog probe requires macOS");
}
