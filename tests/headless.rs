//! Stage 4: caret + mouse selection on the single buffer. These drive the real
//! app through real egui frames and verify the original bug classes are fixed:
//! cross-block mouse selection, whole-doc Ctrl+A, select-then-type replaces.

use eframe::egui::{
    Context, Event, Key, Modifiers, Pos2, PointerButton, RawInput, Rect, Ui, UiBuilder, Vec2,
};
use last_md::app::AppModel;

const SIZE: Vec2 = Vec2::new(900.0, 600.0);

fn ctx() -> Context {
    let c = Context::default();
    last_md::app::setup_fonts(&c);
    c
}

fn raw() -> RawInput {
    let mut r = RawInput::default();
    r.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, SIZE));
    r
}

fn pump(ctx: &Context, app: &mut AppModel, mut raw: RawInput) {
    ctx.begin_pass(raw);
    let mut ui = Ui::new(
        ctx.clone(),
        eframe::egui::Id::new("root"),
        UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, SIZE)),
    );
    app.update(&mut ui);
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
}

/// Pump one frame (to populate last_runs) and return the screen position of the
/// start of the character at buffer byte `b`.
fn char_pos(ctx: &Context, app: &mut AppModel, b: usize) -> Pos2 {
    pump(ctx, app, raw());
    let r = last_md::layout::caret_rect(&app.last_runs, b).expect("painted");
    Pos2::new(r.min.x + 1.0, r.center().y)
}

/// Inject a primary-button press at `pos`.
fn press(ctx: &Context, app: &mut AppModel, pos: Pos2) {
    let mut r = raw();
    r.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    pump(ctx, app, r);
}
/// Inject a drag move to `pos` (button held).
fn drag(ctx: &Context, app: &mut AppModel, pos: Pos2) {
    let mut r = raw();
    r.events.push(Event::PointerMoved(pos));
    pump(ctx, app, r);
}
/// Inject a primary-button release at `pos`.
fn release(ctx: &Context, app: &mut AppModel, pos: Pos2) {
    let mut r = raw();
    r.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    pump(ctx, app, r);
}

fn key(ctx: &Context, app: &mut AppModel, key: Key, mods: Modifiers) {
    let mut r = raw();
    r.events.push(Event::ModifiersChanged(mods));
    r.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods });
    pump(ctx, app, r);
}

fn type_text(ctx: &Context, app: &mut AppModel, text: &str) {
    let mut r = raw();
    r.events.push(Event::Text(text.to_string()));
    pump(ctx, app, r);
}

#[test]
fn ctrl_a_selects_whole_doc() {
    let c = ctx();
    let mut app = AppModel::from_buffer("# Title\n\nBody text here.");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::A, Modifiers::CTRL | Modifiers::COMMAND);
    assert_eq!(app.selection, Some((0, app.buffer.len())), "Ctrl+A selects the whole doc");
}

#[test]
fn select_then_type_replaces() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello world");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::A, Modifiers::CTRL | Modifiers::COMMAND);
    assert!(app.selection.is_some());
    type_text(&c, &mut app, "bye");
    assert_eq!(app.buffer, "bye", "typing over a selection replaces it");
    assert_eq!(app.caret, 3);
}

#[test]
fn caret_moves_across_blocks() {
    let c = ctx();
    // Two paragraphs; caret at the end of the first.
    let mut app = AppModel::from_buffer("first para\n\nsecond para");
    app.caret = "first para".len();
    pump(&c, &mut app, raw());
    // Right arrow should cross the blank line into "second para".
    key(&c, &mut app, Key::ArrowRight, Modifiers::NONE);
    assert!(app.caret > "first para".len(), "caret crossed into the next block (caret={})", app.caret);
    // Left arrow should go back.
    key(&c, &mut app, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(app.caret, "first para".len(), "left arrow returns to first block end");
}

#[test]
fn cross_block_mouse_select() {
    let c = ctx();
    let buf = "alpha block\n\nbeta block";
    let mut app = AppModel::from_buffer(buf);
    // Start of "beta" (byte 14).
    let beta_start = buf.find('b').unwrap();
    let a_pos = char_pos(&c, &mut app, 0);
    let b_pos = char_pos(&c, &mut app, beta_start);
    press(&c, &mut app, a_pos);
    drag(&c, &mut app, b_pos);
    release(&c, &mut app, b_pos);
    let sel = app.selection.expect("cross-block drag produced a selection");
    let (lo, hi) = (sel.0.min(sel.1), sel.0.max(sel.1));
    assert!(hi - lo >= beta_start, "selection spans into the second block (lo={lo} hi={hi}, beta at {beta_start})");
    assert!(lo == 0, "selection starts at the doc start");
}

#[test]
fn escape_clears_selection() {
    let c = ctx();
    let mut app = AppModel::from_buffer("some text");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::A, Modifiers::CTRL | Modifiers::COMMAND);
    assert!(app.selection.is_some());
    key(&c, &mut app, Key::Escape, Modifiers::NONE);
    assert!(app.selection.is_none(), "Escape clears the selection");
}

// --- Stage 5: editing on the buffer --------------------------------------

#[test]
fn backspace_deletes_char() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::Backspace, Modifiers::NONE);
    assert_eq!(app.buffer, "hell");
    assert_eq!(app.caret, 4);
}

#[test]
fn backspace_over_markers_deletes_visible_char() {
    let c = ctx();
    // Caret right after "**b**": the span's markers are revealed (Typora), so
    // Backspace removes the visible closing '*', not the hidden-away 'b'.
    let mut app = AppModel::from_buffer("x **b**");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::Backspace, Modifiers::NONE);
    assert_eq!(app.buffer, "x **b*");
    // Away from a span, Backspace deletes the visible char before the caret
    // and leaves hidden markers alone.
    let mut app = AppModel::from_buffer("**b** cd");
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::Backspace, Modifiers::NONE);
    assert_eq!(app.buffer, "**b** c");
}

#[test]
fn enter_splits_block() {
    let c = ctx();
    let mut app = AppModel::from_buffer("one two");
    app.caret = 3; // after "one"
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::Enter, Modifiers::NONE);
    assert!(app.buffer.contains('\n'), "enter inserted a newline");
    // Splitting "one two" at caret 3 (after "one") yields "one" + " two".
    assert_eq!(app.buffer, "one\n two", "line split at caret");
    // Caret is on the new line.
    assert!(app.caret > 3);
}

#[test]
fn tab_indents_line() {
    let c = ctx();
    let mut app = AppModel::from_buffer("- item");
    app.caret = 6;
    pump(&c, &mut app, raw());
    key(&c, &mut app, Key::Tab, Modifiers::NONE);
    assert!(app.buffer.starts_with("  - "), "tab indented the line, got {:?}", app.buffer);
}

#[test]
fn type_appends_at_caret() {
    let c = ctx();
    let mut app = AppModel::from_buffer("ab");
    app.caret = 1;
    pump(&c, &mut app, raw());
    type_text(&c, &mut app, "X");
    assert_eq!(app.buffer, "aXb");
    assert_eq!(app.caret, 2);
}

// --- Stage 6: undo/redo --------------------------------------------------

#[test]
fn undo_restores_previous() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello");
    pump(&c, &mut app, raw());
    type_text(&c, &mut app, "!");
    assert_eq!(app.buffer, "hello!");
    key(&c, &mut app, Key::Z, Modifiers::CTRL | Modifiers::COMMAND);
    assert_eq!(app.buffer, "hello", "undo restores the previous buffer");
}

#[test]
fn redo_redoes() {
    let c = ctx();
    let mut app = AppModel::from_buffer("hello");
    pump(&c, &mut app, raw());
    type_text(&c, &mut app, "!");
    key(&c, &mut app, Key::Z, Modifiers::CTRL | Modifiers::COMMAND);
    assert_eq!(app.buffer, "hello");
    key(&c, &mut app, Key::Y, Modifiers::CTRL | Modifiers::COMMAND);
    assert_eq!(app.buffer, "hello!", "redo reapplies the edit");
}

#[test]
fn new_edit_clears_redo() {
    let c = ctx();
    let mut app = AppModel::from_buffer("a");
    pump(&c, &mut app, raw());
    type_text(&c, &mut app, "b"); // a -> ab
    key(&c, &mut app, Key::Z, Modifiers::CTRL | Modifiers::COMMAND); // ab -> a
    type_text(&c, &mut app, "c"); // a -> ac (new branch)
    key(&c, &mut app, Key::Y, Modifiers::CTRL | Modifiers::COMMAND); // redo should be a no-op
    assert_eq!(app.buffer, "ac", "a new edit clears the redo stack");
}
