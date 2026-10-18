//! Stage 3: WYSIWYG layout tests. Render the layout and assert markers are
//! hidden, block chrome is present, and inline styles apply.

use eframe::egui::{self, Context, RawInput, Rect, Ui, UiBuilder, Vec2};
use last_md::layout;
use last_md::shot::visible_layout;
use last_md::wysiwyg::scan;

const SIZE: Vec2 = Vec2::new(800.0, 500.0);

/// Render a buffer through the WYSIWYG layout and return the visible runs.
fn render(buffer: &str) -> Vec<last_md::shot::VisRun> {
    let ctx = Context::default();
    last_md::app::setup_fonts(&ctx);
    let atoms = scan(buffer);
    let mut raw = RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(egui::Pos2::ZERO, SIZE));
    ctx.begin_pass(raw);
    let mut ui = Ui::new(
        ctx.clone(),
        egui::Id::new("root"),
        UiBuilder::new().max_rect(Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
    );
    let _ = layout::paint(&mut ui, buffer, &atoms, 0, true, None);
    let mut out = ctx.end_pass();
    let runs = visible_layout(&out);
    out.textures_delta.clear();
    runs
}

fn all_text(runs: &[last_md::shot::VisRun]) -> String {
    runs.iter().map(|r| r.text.clone()).collect()
}

#[test]
fn layout_hides_heading_marker() {
    let runs = render("# Hi");
    let t = all_text(&runs);
    assert!(t.contains("Hi"), "heading text visible: {t}");
    assert!(!t.contains('#'), "heading marker should be hidden: {t}");
    // The heading uses a large font.
    assert!(runs.iter().any(|r| r.text.contains("Hi") && r.font == "InterBold"), "heading should be bold");
}

#[test]
fn layout_hides_list_marker() {
    let runs = render("- item");
    let t = all_text(&runs);
    assert!(t.contains("item"), "list content visible: {t}");
    // The bullet is painted in the gutter; the content sits at the list indent.
    assert!(runs.iter().any(|r| r.text == "item" && r.pos.x >= last_md::layout::LIST_INDENT), "content indented past the bullet gutter");
    assert!(!t.contains("- item"), "list marker hidden: {t}");
}

#[test]
fn layout_hides_quote_marker() {
    let runs = render("> q");
    let t = all_text(&runs);
    assert!(t.contains("q"), "quote content visible: {t}");
    assert!(!t.contains('>'), "quote marker hidden: {t}");
}

#[test]
fn layout_hides_fence() {
    let runs = render("```rust\ncode\n```");
    let t = all_text(&runs);
    assert!(t.contains("code"), "code content visible: {t}");
    assert!(!t.contains("```"), "fence markers hidden: {t}");
    // Code is monospace.
    assert!(runs.iter().any(|r| r.text.contains("code") && r.font == "Monospace"), "code monospace");
}

#[test]
fn layout_inline_bold() {
    let runs = render("a **b** c");
    let t = all_text(&runs);
    assert!(t.contains("b"), "bold content visible: {t}");
    assert!(!t.contains('*'), "bold markers hidden: {t}");
    assert!(runs.iter().any(|r| r.sections.iter().any(|(t, f)| t == "b" && f == "InterBold")), "bold run uses InterBold");
}

#[test]
fn layout_inline_code() {
    let runs = render("x `c` y");
    let t = all_text(&runs);
    assert!(t.contains("c"), "code content visible: {t}");
    assert!(!t.contains('`'), "code markers hidden: {t}");
    assert!(runs.iter().any(|r| r.text == "x c y"), "inline code stays on its line: {t}");
    assert!(runs.iter().any(|r| r.sections.iter().any(|(t, f)| t == "c" && f == "Monospace")), "code monospace");
}

#[test]
fn layout_full_doc_no_markers() {
    let buf = "# Title\n\npara **bold** and `code`\n\n- one\n- two\n\n> quote\n\n```\nfn main() {}\n```";
    let runs = render(buf);
    let t = all_text(&runs);
    // No markdown markers leak into the visible text.
    assert!(!t.contains('#'), "no heading markers: {t}");
    assert!(!t.contains("**"), "no bold markers: {t}");
    assert!(!t.contains('`'), "no code markers: {t}");
    assert!(!t.contains("```"), "no fence markers: {t}");
    // Content present.
    for word in ["Title", "bold", "code", "one", "two", "quote", "fn main() {}"] {
        assert!(t.contains(word), "missing {word} in {t}");
    }
}
