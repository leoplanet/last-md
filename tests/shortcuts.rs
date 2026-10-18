//! Clipboard, universal keyboard shortcuts, mouse word/line selection,
//! scrolling and table editing — driven through real egui frames.

use eframe::egui::{
    self, Context, Event, Key, Modifiers, MouseWheelUnit, OutputCommand, PointerButton, Pos2, Rect, TouchPhase, Ui,
    UiBuilder, Vec2,
};
use last_md::app::AppModel;

struct H {
    ctx: Context,
    app: AppModel,
    t: f64,
    size: Vec2,
}

impl H {
    fn new(buf: &str) -> Self {
        let ctx = Context::default();
        last_md::app::setup_fonts(&ctx);
        let mut h = H { ctx, app: AppModel::from_buffer(buf), t: 0.0, size: Vec2::new(800.0, 600.0) };
        h.frame(vec![]);
        h
    }

    /// One frame; returns the clipboard text egui was asked to copy, if any.
    fn frame(&mut self, events: Vec<Event>) -> Option<String> {
        self.t += 0.05;
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, self.size));
        raw.time = Some(self.t);
        raw.events = events;
        self.ctx.begin_pass(raw);
        let mut ui = Ui::new(self.ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, self.size)));
        self.app.update(&mut ui);
        let mut out = self.ctx.end_pass();
        out.textures_delta.clear();
        out.platform_output.commands.into_iter().find_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Option<String> {
        self.frame(vec![
            Event::ModifiersChanged(modifiers),
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        ])
    }

    fn ctrl(&mut self, key: Key) {
        self.key(key, Modifiers::CTRL | Modifiers::COMMAND);
    }

    fn ctrl_shift(&mut self, key: Key) {
        self.key(key, Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT);
    }

    fn click(&mut self, pos: Pos2) {
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ]);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    }

    /// Screen position inside the painted char at buffer byte `b`.
    fn pos_of(&self, b: usize) -> Pos2 {
        let r = last_md::layout::caret_rect(&self.app.last_runs, b).unwrap();
        Pos2::new(r.min.x + 3.0, r.center().y)
    }

    fn select(&mut self, lo: usize, hi: usize) {
        self.app.selection = Some((lo, hi));
        self.app.caret = hi;
    }
}

// --- clipboard -------------------------------------------------------------

#[test]
fn ctrl_c_copies_selection_markdown() {
    let mut h = H::new("plain **bold** end");
    h.select(6, 14);
    assert_eq!(h.frame(vec![Event::Copy]).as_deref(), Some("**bold**"));
    assert_eq!(h.app.buffer, "plain **bold** end", "copy leaves the text");
}

#[test]
fn ctrl_x_cuts() {
    let mut h = H::new("hello world");
    h.select(5, 11);
    assert_eq!(h.frame(vec![Event::Cut]).as_deref(), Some(" world"));
    assert_eq!(h.app.buffer, "hello");
}

#[test]
fn ctrl_v_pastes_at_caret_and_over_selection() {
    let mut h = H::new("ab");
    h.app.caret = 1;
    h.frame(vec![Event::Paste("XY".into())]);
    assert_eq!(h.app.buffer, "aXYb");
    assert_eq!(h.app.caret, 3);
    h.select(0, 4);
    h.frame(vec![Event::Paste("line1\r\nline2".into())]);
    assert_eq!(h.app.buffer, "line1\nline2", "CRLF normalized, selection replaced");
}

// --- formatting shortcuts --------------------------------------------------

#[test]
fn ctrl_b_wraps_and_unwraps_selection() {
    let mut h = H::new("make this bold");
    h.select(5, 9);
    h.ctrl(Key::B);
    assert_eq!(h.app.buffer, "make **this** bold");
    assert_eq!(h.app.selection, Some((7, 11)), "selection stays on the word");
    h.ctrl(Key::B);
    assert_eq!(h.app.buffer, "make this bold", "Ctrl+B again removes bold");
}

#[test]
fn ctrl_i_and_u_on_word_at_caret() {
    let mut h = H::new("one two");
    h.app.caret = 5; // inside "two"
    h.ctrl(Key::I);
    assert_eq!(h.app.buffer, "one *two*");
    let mut h = H::new("one two");
    h.app.caret = 1;
    h.ctrl(Key::U);
    assert_eq!(h.app.buffer, "<u>one</u> two");
    // Inside the span its tags show (dimmed); away from it they hide.
    let text: String = h.app.last_runs.iter().map(|l| l.galley.text().to_string()).collect();
    assert_eq!(text, "<u>one</u> two", "tags revealed around the caret");
    h.app.caret = h.app.buffer.len();
    h.frame(vec![]);
    let text: String = h.app.last_runs.iter().map(|l| l.galley.text().to_string()).collect();
    assert_eq!(text, "one two", "underline tag hidden");
    assert!(h.app.last_runs[0].galley.job.sections.iter().any(|s| s.format.underline.width > 0.0), "rendered underlined");
}

#[test]
fn ctrl_k_makes_link() {
    let mut h = H::new("see docs");
    h.select(4, 8);
    h.ctrl(Key::K);
    assert_eq!(h.app.buffer, "see [docs]()");
    assert_eq!(h.app.caret, 11, "caret in the url");
}

#[test]
fn ctrl_number_sets_heading() {
    let mut h = H::new("Title");
    h.ctrl(Key::Num2);
    assert_eq!(h.app.buffer, "## Title");
    h.ctrl(Key::Num1);
    assert_eq!(h.app.buffer, "# Title");
    h.ctrl(Key::Num1);
    assert_eq!(h.app.buffer, "Title", "same level again: back to paragraph");
    h.ctrl(Key::Num3);
    h.ctrl(Key::Num0);
    assert_eq!(h.app.buffer, "Title");
}

#[test]
fn redo_with_ctrl_shift_z() {
    let mut h = H::new("a");
    h.frame(vec![Event::Text("b".into())]);
    h.ctrl(Key::Z);
    assert_eq!(h.app.buffer, "a");
    h.ctrl_shift(Key::Z);
    assert_eq!(h.app.buffer, "ab");
}

// --- navigation ------------------------------------------------------------

#[test]
fn ctrl_arrows_move_by_word_over_hidden_markers() {
    let mut h = H::new("one **two** three");
    h.app.caret = 0;
    h.ctrl(Key::ArrowRight);
    assert_eq!(h.app.caret, 6, "to start of 'two' (inside the bold)");
    h.ctrl(Key::ArrowRight);
    assert_eq!(h.app.caret, 12, "to start of 'three'");
    h.ctrl(Key::ArrowLeft);
    assert_eq!(h.app.caret, 6, "back to 'two'");
    h.ctrl(Key::ArrowLeft);
    assert_eq!(h.app.caret, 0);
}

#[test]
fn ctrl_backspace_deletes_word() {
    let mut h = H::new("alpha beta");
    h.ctrl(Key::Backspace);
    assert_eq!(h.app.buffer, "alpha ");
}

#[test]
fn shift_home_end_and_ctrl_home_end() {
    let mut h = H::new("first line\nsecond");
    h.app.caret = 2;
    h.key(Key::End, Modifiers::SHIFT);
    assert_eq!(h.app.selection, Some((2, 10)), "Shift+End selects to line end");
    h.key(Key::Home, Modifiers::SHIFT);
    assert_eq!(h.app.selection, Some((2, 0)), "Shift+Home from the same anchor");
    h.ctrl(Key::End);
    assert_eq!((h.app.caret, h.app.selection), (h.app.buffer.len(), None));
    h.ctrl_shift(Key::Home);
    assert_eq!(h.app.selection, Some((h.app.buffer.len(), 0)), "Ctrl+Shift+Home selects to doc start");
}

#[test]
fn double_click_selects_word_triple_selects_line() {
    let mut h = H::new("alpha **bravo** charlie\nnext");
    let p = h.pos_of(9); // inside "bravo"
    h.click(p);
    h.click(p);
    assert_eq!(h.app.selection, Some((8, 13)), "double-click selects 'bravo'");
    h.click(p);
    assert_eq!(h.app.selection, Some((0, 23)), "triple-click selects the line");
}

#[test]
fn clicking_checkbox_toggles_task() {
    let mut h = H::new("- [ ] todo\n- [x] done");
    let boxes: Vec<Rect> = h.app.last_runs.iter().filter_map(|l| l.checkbox).collect();
    assert_eq!(boxes.len(), 2);
    h.click(boxes[0].center());
    assert_eq!(h.app.buffer, "- [x] todo\n- [x] done");
    h.click(boxes[1].center());
    assert_eq!(h.app.buffer, "- [x] todo\n- [ ] done");
}

// --- scrolling -------------------------------------------------------------

#[test]
fn long_doc_scrolls_and_follows_caret() {
    let doc: String = (1..=200).map(|i| format!("Line {i}\n")).collect();
    let mut h = H::new(&doc);
    h.size = Vec2::new(800.0, 400.0);
    h.app.caret = 0;
    h.frame(vec![]);
    let first_y = |h: &H| h.app.last_runs[0].origin.y;
    let y0 = first_y(&h);
    // Mouse wheel scrolls the document.
    for _ in 0..10 {
        h.frame(vec![
            Event::PointerMoved(Pos2::new(300.0, 200.0)),
            Event::MouseWheel { unit: MouseWheelUnit::Point, delta: Vec2::new(0.0, -120.0), phase: TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE },
        ]);
    }
    for _ in 0..20 {
        h.frame(vec![]);
    }
    assert!(first_y(&h) < y0 - 200.0, "wheel scrolled the doc ({} -> {})", y0, first_y(&h));
    // Ctrl+End jumps to the end and the view follows the caret.
    h.ctrl(Key::End);
    for _ in 0..20 {
        h.frame(vec![]);
    }
    let r = h.app.caret_screen_rect().unwrap();
    assert!(r.min.y >= 0.0 && r.max.y <= 400.0, "caret in view after Ctrl+End: {r:?}");
}

// --- tables ----------------------------------------------------------------

#[test]
fn table_tab_moves_between_cells_and_enter_adds_row() {
    let src = "| a | b |\n|---|---|\n| c | d |";
    let mut h = H::new(src);
    h.app.caret = 2; // in "a"
    h.key(Key::Tab, Modifiers::NONE);
    assert_eq!(h.app.caret, 6, "Tab to 'b'");
    h.key(Key::Tab, Modifiers::NONE);
    assert_eq!(&h.app.buffer[h.app.caret..h.app.caret + 1], "c", "Tab skips the delimiter row to 'c'");
    h.key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(h.app.caret, 6, "Shift+Tab back to 'b'");
    h.app.caret = src.len() - 2; // in "d"
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.app.buffer, format!("{src}\n|  |  |"));
    h.frame(vec![Event::Text("e".into())]);
    assert_eq!(h.app.buffer, format!("{src}\n| e |  |"), "typing lands in the new row's first cell");
}

#[test]
fn ctrl_k_then_type_url() {
    let mut h = H::new("see docs");
    h.select(4, 8);
    h.ctrl(Key::K);
    h.frame(vec![Event::Text("x.dev".into())]);
    assert_eq!(h.app.buffer, "see [docs](x.dev)", "url typed inside the parentheses");
    // Caret leaves the link: it renders as just "docs".
    h.key(Key::Home, Modifiers::NONE);
    let text: String = h.app.last_runs.iter().map(|l| l.galley.text().to_string()).collect();
    assert_eq!(text, "see docs");
}

#[test]
fn shot_reveal_and_demo() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.md")).unwrap();
    let mut h = H::new(&src);
    h.size = Vec2::new(760.0, 1250.0);
    // Caret inside "**bold**" so its markers show.
    h.app.caret = src.find("**bold**").unwrap() + 4;
    h.frame(vec![]);
    let ctx = Context::default();
    last_md::app::setup_fonts(&ctx);
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, h.size));
    ctx.begin_pass(raw);
    let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, h.size)));
    h.app.update(&mut ui);
    let mut out = ctx.end_pass();
    let img = last_md::shot::rasterize(&out, [h.size.x as usize, h.size.y as usize]);
    out.textures_delta.clear();
    let dir = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots");
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
    image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap().save(dir.join("demo_round3.png")).unwrap();
}

#[test]
fn ctrl_f_finds_and_enter_goes_to_next() {
    let mut h = H::new("apple pie, Apple tart, apple juice");
    h.app.caret = 0;
    h.ctrl(Key::F);
    h.frame(vec![]);
    h.frame(vec![Event::Text("apple".into())]);
    assert_eq!(h.app.selection, Some((0, 5)), "first match selected");
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.app.selection, Some((11, 16)), "Enter: next (case-insensitive)");
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.app.selection, Some((23, 28)));
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.app.selection, Some((0, 5)), "wraps around");
    h.key(Key::Enter, Modifiers::SHIFT);
    assert_eq!(h.app.selection, Some((23, 28)), "Shift+Enter: previous");
    assert_eq!(h.app.buffer, "apple pie, Apple tart, apple juice", "typing in the find box doesn't edit the doc");
}

#[test]
fn paste_lands_inline_at_the_caret() {
    for (buf, at, want) in [
        ("Hello world\nSecond", 5, "Hello ASUS TUF world\nSecond"),
        ("Hello world\nSecond", 11, "Hello world ASUS TUF\nSecond"),
        ("- item one\n- two", 6, "- item ASUS TUF one\n- two"),
    ] {
        let mut h = H::new(buf);
        h.app.caret = at;
        h.frame(vec![Event::Paste(" ASUS TUF".into())]);
        assert_eq!(h.app.buffer, want);
        assert_eq!(h.app.caret, at + " ASUS TUF".len());
    }
}

#[test]
fn linux_insert_and_delete_clipboard_keys() {
    // Ctrl+Insert copies; Shift+Delete cuts (egui maps these only on Windows).
    let mut h = H::new("alpha beta");
    h.select(0, 5);
    assert_eq!(h.key(Key::Insert, Modifiers::CTRL | Modifiers::COMMAND).as_deref(), Some("alpha"));
    assert_eq!(h.app.buffer, "alpha beta");
    h.select(0, 6);
    assert_eq!(h.key(Key::Delete, Modifiers::SHIFT).as_deref(), Some("alpha "));
    assert_eq!(h.app.buffer, "beta");
}

fn shot(h: &mut H, name: &str) {
    let ctx = Context::default();
    last_md::app::setup_fonts(&ctx);
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, h.size));
    ctx.begin_pass(raw);
    let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, h.size)));
    h.app.update(&mut ui);
    let mut out = ctx.end_pass();
    let img = last_md::shot::rasterize(&out, [h.size.x as usize, h.size.y as usize]);
    out.textures_delta.clear();
    let dir = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots");
    std::fs::create_dir_all(&dir).unwrap();
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
    image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap().save(dir.join(format!("{name}.png"))).unwrap();
}

fn pixels(h: &mut H) -> egui::ColorImage {
    let ctx = Context::default();
    last_md::app::setup_fonts(&ctx);
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, h.size));
    ctx.begin_pass(raw);
    let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, h.size)));
    h.app.update(&mut ui);
    let mut out = ctx.end_pass();
    let img = last_md::shot::rasterize(&out, [h.size.x as usize, h.size.y as usize]);
    out.textures_delta.clear();
    img
}

#[test]
fn select_all_highlights_every_wrapped_row() {
    let long = "This paragraph is long enough to wrap across several rows of the window. ".repeat(4);
    let mut h = H::new(&format!("{long}\nnext line"));
    h.size = Vec2::new(600.0, 400.0);
    h.frame(vec![]);
    let plain = pixels(&mut h);
    h.ctrl(Key::A);
    let selected = pixels(&mut h);
    shot(&mut h, "select_all");
    let line = &h.app.last_runs[0];
    assert!(line.galley.rows.len() > 2, "wraps");
    for (n, row) in line.galley.rows.iter().enumerate() {
        // The selection tint changes (nearly) every pixel across the row.
        let r = row.rect().translate(line.origin.to_vec2());
        let y = r.center().y as usize;
        let xs = r.min.x as usize..r.max.x as usize;
        let changed = xs.clone().filter(|&x| plain.pixels[y * plain.size[0] + x] != selected.pixels[y * plain.size[0] + x]).count();
        assert!(changed * 10 > xs.len() * 9, "row {n}: {changed}/{} px highlighted", xs.len());
    }
}

#[test]
fn typing_hash_then_space_keeps_the_caret() {
    let mut h = H::new("");
    h.frame(vec![Event::Text("#".into())]);
    // A bare `#` is still text, painted with the caret after it.
    assert_eq!(h.app.last_runs[0].galley.text(), "#");
    let r = last_md::layout::caret_rect(&h.app.last_runs, h.app.caret).expect("caret after #");
    assert!(r.min.x > h.app.last_runs[0].origin.x + 5.0);
    h.frame(vec![Event::Text(" ".into())]);
    // `# ` is an empty h1: marker hidden, caret heading-tall at the text edge.
    let r = last_md::layout::caret_rect(&h.app.last_runs, h.app.caret).expect("caret in empty heading");
    assert!(r.height() > 35.0, "heading-sized caret, got {}", r.height());
    assert!((r.min.x - h.app.last_runs[0].origin.x).abs() < 1.0);
    shot(&mut h, "empty_heading");
    h.frame(vec![Event::Text("T".into())]);
    assert_eq!(h.app.buffer, "# T");
    assert_eq!(h.app.last_runs[0].galley.text(), "T");
}

