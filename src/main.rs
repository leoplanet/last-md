fn main() {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 750.0])
        .with_title("LAST MD")
        // Matches last-md.desktop, so the launcher icon follows the window.
        .with_app_id("last-md");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "LAST MD",
        options,
        Box::new(|cc| {
            last_md::app::setup_fonts(&cc.egui_ctx);
            let mut app = last_md::app::AppModel::new();
            if let Some(path) = std::env::args_os().nth(1) {
                app.open_path(path.into());
            }
            Ok(Box::new(app))
        }),
    )
    .expect("eframe");
}
