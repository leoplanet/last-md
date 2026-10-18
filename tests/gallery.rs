//! Every markdown construct, rendered and checked.
//!
//! * `gallery_shots` renders tests/fixtures/gallery{1..4}.md to PNGs in
//!   $CARGO_TARGET_DIR/shots for eyeballing.
//! * `inline_styles_match_commonmark` checks, char by char, that the visible
//!   text and its bold/italic/strike/code styling match pulldown-cmark's
//!   reading of the same markdown (the CommonMark reference).

use eframe::egui::{self, Context, Pos2, Rect, Ui, UiBuilder, Vec2};
use last_md::app::AppModel;
use last_md::wysiwyg::{Atom, scan};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn gallery_shots() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into())).join("shots");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["gallery1", "gallery2", "gallery3", "gallery4", "review"] {
        let src = fixture(&format!("{name}.md"));
        let size = Vec2::new(760.0, 820.0);
        let mut app = AppModel::from_buffer(&src);
        app.caret = src.len();
        let ctx = Context::default();
        last_md::app::setup_fonts(&ctx);
        let mut out = None;
        for _ in 0..2 {
            let mut raw = egui::RawInput::default();
            raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, size));
            ctx.begin_pass(raw);
            let mut ui = Ui::new(ctx.clone(), egui::Id::new("root"), UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, size)));
            app.update(&mut ui);
            let o = ctx.end_pass();
            // Keep the first frame's output: it carries the whole font atlas.
            if out.is_none() {
                out = Some(o);
            } else {
                let mut o = o;
                o.textures_delta.clear();
            }
        }
        let mut out = out.unwrap();
        let img = last_md::shot::rasterize(&out, [size.x as usize, size.y as usize]);
        out.textures_delta.clear();
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect();
        image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap().save(dir.join(format!("{name}.png"))).unwrap();
    }
}

/// (char, bold, italic, strike, code) for every visible non-space char.
type Styled = Vec<(char, bool, bool, bool, bool)>;

fn reference(md: &str) -> Styled {
    let mut out = Styled::new();
    let (mut b, mut i, mut s) = (0, 0, 0);
    for ev in Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH) {
        match ev {
            Event::Start(Tag::Strong) => b += 1,
            Event::End(TagEnd::Strong) => b -= 1,
            Event::Start(Tag::Emphasis) | Event::Start(Tag::Image { .. }) => i += 1,
            Event::End(TagEnd::Emphasis) | Event::End(TagEnd::Image) => i -= 1,
            Event::Start(Tag::Strikethrough) => s += 1,
            Event::End(TagEnd::Strikethrough) => s -= 1,
            Event::Text(t) => out.extend(t.chars().filter(|c| !c.is_whitespace()).map(|c| (c, b > 0, i > 0, s > 0, false))),
            Event::Code(t) => out.extend(t.chars().filter(|c| !c.is_whitespace()).map(|c| (c, b > 0, i > 0, s > 0, true))),
            _ => {}
        }
    }
    out
}

fn ours(md: &str) -> Styled {
    let mut out = Styled::new();
    for a in scan(md) {
        if let Atom::Text { range, style, .. } = a {
            out.extend(md[range].chars().filter(|c| !c.is_whitespace()).map(|c| (c, style.bold, style.italic, style.strike, style.code)));
        }
    }
    out
}

#[test]
fn inline_styles_match_commonmark() {
    // One case per paragraph; headings are bold by design, so only body text.
    let cases = [
        "Plain *italic* and _italic_ and **bold** and __bold__.",
        "Mixed ***bold italic*** and ___bold italic___ here.",
        "Nested **bold with *italic* inside** and *italic with **bold** inside*.",
        "Strike ~~through~~ and `inline code` and ``code with ` tick``.",
        "Not emphasis: snake_case_word, 2 * 3 * 4, ** spaced **.",
        "Unclosed: *open only.",
        "Intraword: un*frigging*believable and un**frigging**believable.",
        "Escapes: \\*not italic\\*, \\# not heading, \\`not code\\`.",
        "Link [text](https://example.com) and [**bold** link](https://x.dev \"T\").",
        "Image ![alt *text*](pic.png) inline.",
        "*This * is the first good version.",
        "** bold ** and --bold-- are not bold.",
        "**a** **b** *c**d**e* ***f** g*",
        "*a\nb* across a soft break, **c\nd** too.",
        "`code with **stars**` and **bold `code` bold**.",
        "_a_b and a_b_ and *a*b and a*b*.",
        "****four**** and **unclosed *mixed",
        "- list *item* with **bold**",
        "> quote *item* with **bold**",
        "1. ordered ~~strike~~ `code`",
    ];
    let mut bad = Vec::new();
    for md in cases {
        let (want, got) = (reference(md), ours(md));
        if want != got {
            bad.push(format!("{md:?}\n  want {want:?}\n  got  {got:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
