//! Visual caret probes: render real app frames to PNG (in $CARGO_TARGET_DIR/shots)
//! so caret placement can be inspected by eye, and assert the caret is painted
//! where the text says it should be.

use eframe::egui::{self, ColorImage, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, Vec2};
use last_md::app::AppModel;
use last_md::shot::rasterize;

const SIZE: Vec2 = Vec2::new(640.0, 260.0);

fn ctx() -> Context {
    let c = Context::default();
    last_md::app::setup_fonts(&c);
    c
}

fn frame(ctx: &Context, app: &mut AppModel, events: Vec<Event>) {
    let mut out = frame_out(ctx, app, events);
    out.textures_delta.clear();
}

fn frame_out(ctx: &Context, app: &mut AppModel, events: Vec<Event>) -> egui::FullOutput {
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, SIZE));
    raw.events = events;
    ctx.begin_pass(raw);
    let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, SIZE)));
    app.update(&mut ui);
    ctx.end_pass()
}

fn key(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn click(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]
}

fn release(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]
}

fn save(img: &ColorImage, name: &str) {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots");
    std::fs::create_dir_all(&dir).unwrap();
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
    image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap().save(dir.join(format!("{name}.png"))).unwrap();
}

/// Render the app (two frames so the font atlas is complete) and save a PNG.
fn shoot(_ctx: &Context, app: &mut AppModel, name: &str) {
    // A fresh context's first frame carries the whole font atlas.
    let mut out = frame_out(&ctx(), app, vec![]);
    let img = rasterize(&out, [SIZE.x as usize, SIZE.y as usize]);
    out.textures_delta.clear();
    save(&img, name);
}

/// Screen rect of the caret as painted for the app's current caret.
fn caret_rect(app: &AppModel) -> Rect {
    app.caret_screen_rect().expect("caret painted")
}

/// Screen x just after the visible text `needle` on the line containing it.
fn x_after(app: &AppModel, needle: &str) -> f32 {
    for line in &app.last_runs {
        let text = line.galley.text();
        if let Some(i) = text.find(needle) {
            let chars = text[..i + needle.len()].chars().count();
            return line.origin.x + line.galley.pos_from_cursor(egui::text::CCursor::new(chars)).min.x;
        }
    }
    panic!("{needle:?} not painted");
}

fn y_of(app: &AppModel, needle: &str) -> f32 {
    app.last_runs.iter().find(|l| l.galley.text().contains(needle)).map(|l| l.origin.y).expect("painted")
}

#[test]
fn caret_at_doc_end_sits_after_last_char() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello");
    frame(&c, &mut app, vec![]);
    shoot(&c, &mut app, "caret_doc_end");
    let r = caret_rect(&app);
    assert!((r.min.x - x_after(&app, "hello")).abs() < 1.5, "caret x {} vs end of 'hello' {}", r.min.x, x_after(&app, "hello"));
}

#[test]
fn end_key_puts_caret_at_end_of_same_line() {
    let c = ctx();
    let mut app = AppModel::from_buffer("first line\nsecond");
    app.caret = 0;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::End)]);
    shoot(&c, &mut app, "caret_end_key");
    assert_eq!(app.caret, "first line".len());
    let r = caret_rect(&app);
    assert!((r.min.y - y_of(&app, "first")).abs() < 1.5, "caret on first line");
    assert!((r.min.x - x_after(&app, "first line")).abs() < 1.5, "caret after 'first line'");
}

#[test]
fn right_arrow_reaches_line_end() {
    let c = ctx();
    let mut app = AppModel::from_buffer("ab\ncd");
    app.caret = 1;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::ArrowRight)]);
    assert_eq!(app.caret, 2, "right arrow stops at end of line before wrapping");
}

#[test]
fn inline_markers_stay_on_one_line() {
    let c = ctx();
    let mut app = AppModel::from_buffer("x `code` and *it* and [link](u) end");
    frame(&c, &mut app, vec![]);
    shoot(&c, &mut app, "inline_one_line");
    assert_eq!(app.last_runs.len(), 1, "one painted line, got {}", app.last_runs.len());
}

#[test]
fn caret_in_list_item_after_text() {
    let c = ctx();
    let mut app = AppModel::from_buffer("- a **b** c");
    frame(&c, &mut app, vec![]);
    shoot(&c, &mut app, "caret_list_end");
    let r = caret_rect(&app);
    assert!((r.min.x - x_after(&app, "a b c")).abs() < 1.5, "caret x {} vs after text {}", r.min.x, x_after(&app, "a b c"));
}

#[test]
fn empty_list_item_keeps_bullet_and_caret() {
    let c = ctx();
    let mut app = AppModel::from_buffer("- one");
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::Enter)]);
    shoot(&c, &mut app, "caret_new_list_item");
    assert_eq!(app.buffer, "- one\n- ");
    let r = caret_rect(&app);
    assert!(r.min.y > y_of(&app, "one") + 5.0, "caret on the new item line");
    let first = app.last_runs.iter().find(|l| l.galley.text() == "one").unwrap();
    let new_item = app.last_runs.iter().find(|l| l.contains(app.caret)).unwrap();
    assert_eq!(new_item.origin.x, first.origin.x, "new empty item sits in the list's content column");
    assert!((r.min.x + 1.0 - first.origin.x).abs() < 1.0, "caret at the content column");
}

#[test]
fn click_past_line_end_places_caret_at_that_line_end() {
    let c = ctx();
    let mut app = AppModel::from_buffer("short\nlonger line");
    app.caret = 0;
    frame(&c, &mut app, vec![]);
    let y = y_of(&app, "short") + 8.0;
    frame(&c, &mut app, click(Pos2::new(500.0, y)));
    frame(&c, &mut app, release(Pos2::new(500.0, y)));
    assert_eq!(app.caret, "short".len(), "click right of 'short' -> end of that line");
    let y2 = y_of(&app, "longer") + 8.0;
    frame(&c, &mut app, click(Pos2::new(500.0, y2)));
    frame(&c, &mut app, release(Pos2::new(500.0, y2)));
    assert_eq!(app.caret, app.buffer.len(), "click right of last line -> doc end (not doc start)");
}

#[test]
fn caret_repaints_same_frame_as_keypress() {
    let c = ctx();
    let mut app = AppModel::from_buffer("abc");
    app.caret = 0;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::End)]);
    // No further frame: what's on screen must already show the moved caret.
    let r = caret_rect(&app);
    assert!((r.min.x - x_after(&app, "abc")).abs() < 1.5, "caret drawn at new position in the same frame");
}

#[test]
fn up_down_move_between_lines() {
    let c = ctx();
    let mut app = AppModel::from_buffer("abcdef\nxy\nlast line");
    app.caret = 4;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::ArrowDown)]);
    assert_eq!(app.caret, "abcdef\nxy".len(), "down onto shorter line clamps to its end");
    frame(&c, &mut app, vec![key(Key::ArrowDown)]);
    assert!(app.caret > "abcdef\nxy\n".len(), "down to third line");
    frame(&c, &mut app, vec![key(Key::ArrowUp), key(Key::ArrowUp)]);
    assert!(app.caret <= 6, "back up to first line");
}

#[test]
fn shift_arrow_extends_selection() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello");
    app.caret = 0;
    frame(&c, &mut app, vec![]);
    let shift_right = Event::Key { key: Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT };
    frame(&c, &mut app, vec![Event::ModifiersChanged(Modifiers::SHIFT), shift_right.clone()]);
    frame(&c, &mut app, vec![shift_right]);
    assert_eq!(app.selection, Some((0, 2)));
    shoot(&c, &mut app, "shift_select");
}

#[test]
fn opened_doc_caret_skips_heading_marker() {
    let c = ctx();
    let mut app = AppModel::new();
    app.load_doc("# Title");
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![Event::Text("X".into())]);
    assert_eq!(app.buffer, "# XTitle", "typing at load caret must not break the heading marker");
}

#[test]
fn backspace_at_line_start_joins_lines() {
    let c = ctx();
    let mut app = AppModel::from_buffer("ab\ncd");
    app.caret = 3;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::Backspace)]);
    assert_eq!(app.buffer, "abcd", "joins lines, keeps the 'b'");
    assert_eq!(app.caret, 2);
}

#[test]
fn tab_indented_item_stays_a_list_item() {
    let c = ctx();
    let mut app = AppModel::from_buffer("- a\n- b");
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::Tab)]);
    assert_eq!(app.buffer, "- a\n  - b");
    frame(&c, &mut app, vec![key(Key::Enter)]);
    assert_eq!(app.buffer, "- a\n  - b\n  - ", "Enter continues at the nested depth");
    let text: String = app.last_runs.iter().map(|l| l.galley.text().to_string()).collect();
    assert!(!text.contains('-'), "nested marker hidden: {text:?}");
    shoot(&c, &mut app, "nested_list");
}

#[test]
fn demo_doc_caret_in_code_block() {
    let c = ctx();
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.md")).unwrap();
    let mut app = AppModel::from_buffer(&src);
    let Some(i) = src.find("```") else { return };
    let code_line = src[i..].find('\n').unwrap() + i + 1;
    app.caret = code_line + 4;
    frame(&c, &mut app, vec![]);
    let r = caret_rect(&app);
    let line = app.last_runs.iter().find(|l| l.contains(app.caret)).unwrap();
    assert!(r.min.y >= line.origin.y - 0.5 && r.max.y <= line.origin.y + line.height + 0.5, "caret on the code line");
    // Tall shot of the whole demo.
    let mut big = egui::RawInput::default();
    let size = Vec2::new(760.0, 1100.0);
    big.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, size));
    let fresh = ctx();
    fresh.begin_pass(big);
    let mut ui = Ui::new(fresh.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, size)));
    app.update(&mut ui);
    let mut out = fresh.end_pass();
    let img = rasterize(&out, [size.x as usize, size.y as usize]);
    out.textures_delta.clear();
    save(&img, "demo_full");
}

#[test]
fn caret_visible_on_open() {
    let c = ctx();
    // Fresh app, empty doc.
    let mut app = AppModel::new();
    frame(&c, &mut app, vec![]);
    let r = caret_rect(&app);
    assert!(r.height() > 10.0, "empty doc caret visible, got {r:?}");
    shoot(&c, &mut app, "open_empty");
    // Opened file.
    let mut app = AppModel::new();
    app.open_path(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.md").into());
    frame(&c, &mut app, vec![]);
    let r = caret_rect(&app);
    assert!(r.height() > 10.0, "opened doc caret visible, got {r:?}");
    shoot(&c, &mut app, "open_demo");
}

// --- code blocks: getting in and out --------------------------------------

fn typed(c: &Context, app: &mut AppModel, s: &str) {
    frame(c, app, vec![Event::Text(s.into())]);
}

fn ctrl(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL | Modifiers::COMMAND }
}

#[test]
fn unclosed_fence_shows_as_text() {
    let c = ctx();
    let mut app = AppModel::from_buffer("```");
    frame(&c, &mut app, vec![]);
    assert!(app.last_runs.iter().any(|l| l.galley.text().contains("```")), "typed ``` stays visible until Enter");
}

#[test]
fn fence_enter_opens_block_and_enter_twice_exits() {
    let c = ctx();
    let mut app = AppModel::new();
    frame(&c, &mut app, vec![]);
    typed(&c, &mut app, "```rust");
    frame(&c, &mut app, vec![key(Key::Enter)]);
    assert_eq!(app.buffer, "```rust\n\n```");
    assert_eq!(app.caret, 8, "caret on the block's first line");
    typed(&c, &mut app, "fn main() {}");
    frame(&c, &mut app, vec![key(Key::Enter)]);
    typed(&c, &mut app, "x");
    shoot(&c, &mut app, "code_inside");
    frame(&c, &mut app, vec![key(Key::Enter)]);
    frame(&c, &mut app, vec![key(Key::Enter)]);
    assert_eq!(app.buffer, "```rust\nfn main() {}\nx\n```\n", "empty last line dropped");
    assert_eq!(app.caret, app.buffer.len(), "caret after the block");
    typed(&c, &mut app, "after");
    assert!(app.buffer.ends_with("```\nafter"));
    shoot(&c, &mut app, "code_exited");
}

#[test]
fn arrows_and_ctrl_enter_leave_block_at_doc_end() {
    for (name, ev) in [("down", key(Key::ArrowDown)), ("right", key(Key::ArrowRight)), ("ctrl_enter", ctrl(Key::Enter))] {
        let c = ctx();
        let mut app = AppModel::from_buffer("```\ncode\n```");
        app.caret = 8; // end of "code"
        frame(&c, &mut app, vec![]);
        frame(&c, &mut app, vec![ev]);
        assert_eq!(app.buffer, "```\ncode\n```\n", "{name}: a line is added after the block");
        assert_eq!(app.caret, app.buffer.len(), "{name}: caret below the block");
        let r = caret_rect(&app);
        let code_line = app.last_runs.iter().find(|l| l.galley.text() == "code").unwrap();
        assert!(r.min.y > code_line.origin.y + code_line.height, "{name}: caret painted below the code");
    }
}

#[test]
fn down_into_next_paragraph_from_block() {
    let c = ctx();
    let mut app = AppModel::from_buffer("```\ncode\n```\nnext");
    app.caret = 6;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::ArrowDown)]);
    assert!(app.caret >= "```\ncode\n```\n".len(), "down leaves into 'next'");
    assert_eq!(app.buffer, "```\ncode\n```\nnext", "no line added when one exists");
}

#[test]
fn backspace_removes_empty_block() {
    let c = ctx();
    let mut app = AppModel::from_buffer("para\n```\n\n```");
    app.caret = 9;
    frame(&c, &mut app, vec![]);
    frame(&c, &mut app, vec![key(Key::Backspace)]);
    assert_eq!(app.buffer, "para\n");
}

#[test]
fn heading_sizes_match_typora() {
    let c = ctx();
    let mut app = AppModel::from_buffer("# H1\n## H2\n### H3\n#### H4\nbody");
    frame(&c, &mut app, vec![]);
    shoot(&c, &mut app, "headings");
    let h = |t: &str| app.last_runs.iter().find(|l| l.galley.text() == t).unwrap().galley.job.sections[0].format.font_id.size;
    assert_eq!((h("H1") / h("body"), h("H2") / h("body"), h("H3") / h("body"), h("H4") / h("body")), (2.25, 1.75, 1.5, 1.25));
}
