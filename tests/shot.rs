//! Stage 0: visual harness tests. Render the real app to a PNG and verify we
//! can see it (non-blank), that the file is a valid PNG, and that we can
//! extract the visible layout for assertions.

use eframe::egui::{self, ColorImage, Context, Event, Pos2, Rect, Ui, UiBuilder, Vec2};
use last_md::app::AppModel;
use last_md::shot::{rasterize, visible_layout};

const SIZE: Vec2 = Vec2::new(800.0, 500.0);

fn make_ctx() -> Context {
    let ctx = Context::default();
    last_md::app::setup_fonts(&ctx);
    ctx
}

/// Pump one frame WITHOUT clearing textures_delta (so rasterize can read the
/// font atlas).
fn pump(app: &mut AppModel, ctx: &Context, events: Vec<Event>) -> egui::FullOutput {
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, SIZE));
    raw.events = events;
    ctx.begin_pass(raw);
    let mut ui = Ui::new(
        ctx.clone(),
        egui::Id::new("root"),
        UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, SIZE)),
    );
    app.update(&mut ui);
    ctx.end_pass()
}

fn text(s: &str) -> Event {
    Event::Text(s.to_string())
}

fn shots_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots")
}

/// Save a ColorImage as a PNG (image crate is a dev-dependency).
fn save_png(img: &ColorImage, dir: &std::path::Path, name: &str) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{name}.png"));
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
    let buf = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad buffer"))?;
    buf.save(&path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("save: {e}")))?;
    Ok(path)
}

#[test]
fn shot_saves_file() {
    let ctx = make_ctx();
    let mut app = AppModel::new();
    app.buffer = "hello world".to_string();
    { let mut o = pump(&mut app, &ctx, vec![]); o.textures_delta.clear(); }
    let mut out = pump(&mut app, &ctx, vec![]);
    let img = rasterize(&out, [SIZE.x as usize, SIZE.y as usize]);
    out.textures_delta.clear();
    let path = save_png(&img, &shots_dir(), "s0_smoke").expect("save png");
    assert!(path.exists(), "png should exist at {path:?}");
    let bytes = std::fs::read(&path).expect("read png");
    // PNG signature.
    assert_eq!(&bytes[0..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}

#[test]
fn shot_is_nonblank() {
    let ctx = make_ctx();
    let mut app = AppModel::new();
    app.buffer = "hello world".to_string();
    { let mut o = pump(&mut app, &ctx, vec![]); o.textures_delta.clear(); }
    let mut out = pump(&mut app, &ctx, vec![]);
    let img = rasterize(&out, [SIZE.x as usize, SIZE.y as usize]);
    out.textures_delta.clear();
    // Count pixels that differ from the background (18,18,18).
    let nonbg = img
        .pixels
        .iter()
        .filter(|c| {
            let d = |a: u8, b: u8| a.abs_diff(b) as u32;
            d(c.r(), 18) + d(c.g(), 18) + d(c.b(), 18) > 24
        })
        .count();
    assert!(
        nonbg > 100,
        "expected rendered text pixels, got {nonbg} non-background pixels"
    );
}

#[test]
fn visible_layout_extracts_text() {
    let ctx = make_ctx();
    let mut app = AppModel::new();
    app.buffer = "hello world".to_string();
    { let mut o = pump(&mut app, &ctx, vec![]); o.textures_delta.clear(); }
    let mut out = pump(&mut app, &ctx, vec![]);
    let runs = visible_layout(&out);
    out.textures_delta.clear();
    assert!(
        runs.iter().any(|r| r.text.contains("hello world")),
        "expected a visible run with 'hello world', got {:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
}
