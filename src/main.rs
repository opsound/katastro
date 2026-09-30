use eframe::{
    egui,
    egui_wgpu::{WgpuSetup, WgpuSetupCreateNew, wgpu},
};
use katastro::{desktop::DesktopApp, engine::EngineConfig};
use std::path::PathBuf;
fn main() -> eframe::Result {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let data = std::env::var_os("KATASTRO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Library/Application Support/Katastro"));
    let cache = std::env::var_os("KATASTRO_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Library/Caches/Katastro"));
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    if cfg!(target_os = "macos") {
        setup.instance_descriptor.backends = wgpu::Backends::METAL;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Katastro — Go game review")
            .with_inner_size([1200.0, 860.0])
            .with_min_inner_size([900.0, 680.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Katastro",
        options,
        Box::new(move |cc| {
            if let Some(render) = &cc.wgpu_render_state {
                let info = render.adapter.get_info();
                eprintln!("Katastro renderer: {:?} · {}", info.backend, info.name);
            }
            Ok(Box::new(DesktopApp::new(
                &cc.egui_ctx,
                data.join("reviews.sqlite"),
                cache.join("analysis.sqlite"),
                EngineConfig::discover(&home),
                initial,
            )))
        }),
    )
}
