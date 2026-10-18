//! Left-edge alignment of every markdown construct. Each case renders under a
//! plain reference paragraph; the first visible glyph of every line is
//! measured exactly from the laid-out galley. Run with `--nocapture` to see
//! the full table; a PNG of all cases lands in $CARGO_TARGET_DIR/shots.

use eframe::egui::{self, Context, Pos2, Rect, Ui, UiBuilder, Vec2};
use last_md::app::AppModel;
use last_md::layout::LinePaint;
use last_md::shot::rasterize;

const SIZE: Vec2 = Vec2::new(700.0, 3600.0);

/// (label, markdown, expected left-edge class)
/// "text": content must align with the paragraph text column.
/// "list": content must align with other list items' content column.
/// "nested": content must align with other nested items' content column.
/// "quote": content must align with other quote lines.
/// "code": content must align with other code lines.
const CASES: &[(&str, &str, &str)] = &[
    ("plain", "Word and more", "text"),
    ("bold **", "**Word** and more", "text"),
    ("bold __", "__Word__ and more", "text"),
    ("italic *", "*Word* and more", "text"),
    ("italic _", "_Word_ and more", "text"),
    ("bold-italic ***", "***Word*** and more", "text"),
    ("code `", "`Word` and more", "text"),
    ("code ``", "``Word`` and more", "text"),
    ("strike ~~", "~~Word~~ and more", "text"),
    ("link", "[Word](https://x.dev) and more", "text"),
    ("image", "![Word](a.png) and more", "text"),
    ("autolink", "<https://x.dev> and more", "text"),
    ("escaped", "\\*Word\\* and more", "text"),
    ("html", "<b>Word</b> and more", "text"),
    ("leading spaces", "   Word and more", "text"),
    ("h1", "# Word", "text"),
    ("h2", "## Word", "text"),
    ("h3", "### Word", "text"),
    ("h4", "#### Word", "text"),
    ("h5", "##### Word", "text"),
    ("h6", "###### Word", "text"),
    ("h1 bold", "# **Word**", "text"),
    ("h2 code", "## `Word`", "text"),
    ("bullet -", "- Word", "list"),
    ("bullet *", "* Word", "list"),
    ("bullet +", "+ Word", "list"),
    ("bullet bold", "- **Word**", "list"),
    ("bullet italic", "- *Word*", "list"),
    ("bullet code", "- `Word`", "list"),
    ("ordered 1.", "1. Word", "list"),
    ("ordered 9.", "9. Word", "list"),
    ("ordered 10.", "10. Word", "list"),
    ("ordered 1)", "1) Word", "list"),
    ("task [ ]", "- [ ] Word", "list"),
    ("task [x]", "- [x] Word", "list"),
    ("nested -", "  - Word", "nested"),
    ("nested bold", "  - **Word**", "nested"),
    ("nested 1.", "  1. Word", "nested"),
    ("quote", "> Word", "quote"),
    ("quote bold", "> **Word**", "quote"),
    ("quote italic", "> *Word*", "quote"),
    ("quote no space", ">Word", "quote"),
    ("code block", "```\nWord\n```", "code"),
    ("code block indented", "```\n  Word\n```", "codeindent"),
    ("html u", "<u>Word</u> and more", "text"),
    ("html b", "<b>Word</b> and more", "text"),
    ("literal ***", "*** and more", "text"),
    ("hr then text", "---\nWord", "text"),
    ("table", "| Word | b |\n|---|---|\n| c | d |", "cell"),
    ("table aligned", "| Word | b |\n|:--|--:|\n| c | d |", "cell"),
];

fn ctx() -> Context {
    let c = Context::default();
    last_md::app::setup_fonts(&c);
    c
}

fn render(ctx: &Context, app: &mut AppModel, size: Vec2) -> egui::FullOutput {
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, size));
    ctx.begin_pass(raw);
    let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, size)));
    app.update(&mut ui);
    ctx.end_pass()
}

/// (logical x, ink x) of the first visible (non-space) glyph on a line.
fn first_glyph_x(line: &LinePaint) -> Option<(f32, f32, char)> {
    let row = line.galley.rows.first()?;
    // Skip the list bullet prefix and whitespace.
    let g = row.glyphs.iter().find(|g| !g.chr.is_whitespace())?;
    let x = line.origin.x + row.pos.x + g.pos.x;
    Some((x, x + g.uv_rect.offset.x, g.chr))
}

/// Measure the content left edge of the case line (the one after "Ref").
fn measure(md: &str) -> (f32, f32, char, String) {
    let c = ctx();
    let mut app = AppModel::from_buffer(&format!("Ref line\n\n{md}"));
    // Caret away from the case, so no inline markers are revealed.
    app.caret = 0;
    let mut out = render(&c, &mut app, Vec2::new(700.0, 400.0));
    out.textures_delta.clear();
    let line = app
        .last_runs
        .iter()
        .filter(|l| l.buf_start >= "Ref line\n\n".len())
        .find(|l| first_glyph_x(l).is_some())
        .expect("case painted");
    let (x, ink, ch) = first_glyph_x(line).unwrap();
    (x, ink, ch, line.galley.text().to_string())
}

#[test]
fn alignment_table() {
    let (ref_x, ref_ink, _, _) = measure("Word and more");
    let mut classes: std::collections::BTreeMap<&str, Vec<(&str, f32, f32)>> = Default::default();
    println!("\n{:<22} {:>8} {:>8}  first  shown", "case", "x", "ink");
    for (label, md, class) in CASES {
        let (x, ink, ch, shown) = measure(md);
        println!("{label:<22} {:>8.2} {:>8.2}  {ch:?}    {shown:?}", x - ref_x, ink - ref_ink);
        classes.entry(class).or_default().push((label, x, ink));
    }
    let mut bad = Vec::new();
    for (class, items) in &classes {
        let (bx, bink) = match *class {
            "text" => (ref_x, ref_ink),
            // Table border on the text edge, cell text padded inside it.
            "cell" => (ref_x + last_md::layout::CELL_PAD, ref_ink + last_md::layout::CELL_PAD),
            _ => (items[0].1, items[0].2),
        };
        for (label, x, ink) in items {
            if (x - bx).abs() > 0.5 || (ink - bink).abs() > 1.5 {
                bad.push(format!("{class}/{label}: x {:+.2} ink {:+.2}", x - bx, ink - bink));
            }
        }
    }
    // Snapshot of all cases for eyeballing.
    let all: String = CASES.iter().map(|(l, md, _)| format!("Plain {l}\n{md}\n\n")).collect();
    let c = ctx();
    let mut app = AppModel::from_buffer(&all);
    app.caret = 0;
    let mut out = render(&c, &mut app, SIZE);
    let img = rasterize(&out, [SIZE.x as usize, SIZE.y as usize]);
    out.textures_delta.clear();
    let dir = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots");
    std::fs::create_dir_all(&dir).unwrap();
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
    image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap().save(dir.join("alignment.png")).unwrap();
    assert!(bad.is_empty(), "misaligned:\n{}", bad.join("\n"));
}

/// Content x of the line showing `needle`.
fn content_x(app: &AppModel, needle: &str) -> f32 {
    let line = app.last_runs.iter().find(|l| l.galley.text().contains(needle)).unwrap_or_else(|| panic!("{needle} not painted"));
    first_glyph_x(line).unwrap().0
}

#[test]
fn list_items_share_one_content_column() {
    let c = ctx();
    let mut app = AppModel::from_buffer(
        "- alpha\n- **bravo**\n- *charlie*\n- [ ] delta\n- [x] echo\n\n9. foxtrot\n10. golf\n100. hotel\n\n- india\n  - juliet\n  - **kilo**\n  1. lima\n    - mike",
    );
    let mut out = render(&c, &mut app, Vec2::new(700.0, 800.0));
    out.textures_delta.clear();
    let col = content_x(&app, "alpha");
    for w in ["bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india"] {
        assert!((content_x(&app, w) - col).abs() < 0.5, "{w} at {} vs {col}", content_x(&app, w));
    }
    let nested = content_x(&app, "juliet");
    assert!((nested - col - last_md::layout::LIST_INDENT).abs() < 0.5, "nested one indent in");
    for w in ["kilo", "lima"] {
        assert!((content_x(&app, w) - nested).abs() < 0.5, "{w} with juliet");
    }
    assert!((content_x(&app, "mike") - nested - last_md::layout::LIST_INDENT).abs() < 0.5);
    // A list numbers on from its first item (CommonMark): 9, 10, 11; a
    // nested numbered list after bullets starts again at its own number.
    let numbers: Vec<String> = out_numbers(&c, &mut app);
    for n in ["9.", "10.", "11.", "1."] {
        assert!(numbers.iter().any(|t| t == n), "number {n} shown: {numbers:?}");
    }
}

fn out_numbers(c: &Context, app: &mut AppModel) -> Vec<String> {
    let mut out = render(c, app, Vec2::new(700.0, 800.0));
    out.textures_delta.clear();
    last_md::shot::visible_layout(&out).into_iter().map(|r| r.text).collect()
}

#[test]
fn wrapped_rows_hang_under_the_text() {
    let long = "word ".repeat(60);
    for (md, label) in [(format!("- {long}"), "list"), (format!("> {long}"), "quote"), (format!("1. {long}"), "ordered"), (format!("  - {long}"), "nested")] {
        let c = ctx();
        let mut app = AppModel::from_buffer(&md);
        let mut out = render(&c, &mut app, Vec2::new(700.0, 800.0));
        out.textures_delta.clear();
        let line = &app.last_runs[0];
        assert!(line.galley.rows.len() > 1, "{label} wraps");
        let xs: Vec<f32> = line.galley.rows.iter().map(|r| line.origin.x + r.pos.x + r.glyphs[0].pos.x).collect();
        assert!(xs.iter().all(|x| (x - xs[0]).abs() < 0.5), "{label}: wrapped rows at {xs:?}");
    }
}

#[test]
fn table_columns_line_up() {
    let c = ctx();
    let mut app = AppModel::from_buffer("| Name | Qty |\n|---|---|\n| apple | 3 |\n| watermelon | 12 |");
    let mut out = render(&c, &mut app, Vec2::new(700.0, 400.0));
    out.textures_delta.clear();
    let x_of = |needle: &str| {
        let line = app.last_runs.iter().find(|l| l.galley.text().contains(needle)).unwrap();
        let idx = line.galley.text().find(needle).unwrap();
        let chars = line.galley.text()[..idx].chars().count();
        line.origin.x + line.galley.pos_from_cursor(egui::text::CCursor::new(chars)).min.x
    };
    assert!((x_of("Name") - x_of("apple")).abs() < 0.5 && (x_of("apple") - x_of("watermelon")).abs() < 0.5, "first column");
    assert!((x_of("Qty") - x_of("3")).abs() < 0.5 && (x_of("3") - x_of("12")).abs() < 0.5, "second column");
    assert!(x_of("Qty") > x_of("watermelon") + 50.0, "second column right of the widest first cell");
}
