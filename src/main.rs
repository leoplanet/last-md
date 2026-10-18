fn main() {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_title("Last-MD"),
        ..Default::default()
    };
    eframe::run_native(
        "Last-MD",
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
