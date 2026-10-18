//! Markdown model: block classification, inline spans, smart-input rules.
//! Pure logic, no GUI — unit-tested.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
    pub underline: bool,
    /// A markdown marker revealed around the caret (drawn dimmed).
    pub marker: bool,
}

impl Style {
    pub const NONE: Style = Style { bold: false, italic: false, strike: false, code: false, link: false, underline: false, marker: false };
    pub fn merge(self, other: Style) -> Style {
        Style {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            strike: self.strike || other.strike,
            code: self.code || other.code,
            link: self.link || other.link,
            underline: self.underline || other.underline,
            marker: self.marker || other.marker,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// Some(url) for links.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub task: Option<bool>,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Empty,
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Code(String, String), // (lang, code)
    Quote(Vec<Span>),
    List { ordered: bool, items: Vec<ListItem> },
    Table {
        align: Vec<Alignment>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Hr,
    /// Raw text fallback (HTML blocks, metadata, anything unhandled).
    Raw(String),
}

fn opts() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

pub fn classify(src: &str) -> Block {
    if src.trim().is_empty() {
        return Block::Empty;
    }
    let mut events = Parser::new_ext(src, opts()).peekable();
    while let Some(ev) = events.next() {
        match ev {
            Event::Start(tag) => return classify_tag(src, tag, &mut events),
            Event::Rule => return Block::Hr,
            _ => {}
        }
    }
    Block::Empty
}

fn classify_tag<'a>(src: &str, tag: Tag<'a>, events: &mut impl Iterator<Item = Event<'a>>) -> Block {
    match tag {
        Tag::Heading { level, .. } => Block::Heading(level as u8, inline_spans(events, None)),
        Tag::Paragraph => Block::Paragraph(inline_spans(events, None)),
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) => l.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            let mut code = String::new();
            for ev in events {
                match ev {
                    Event::Text(t) => code.push_str(&t),
                    Event::End(TagEnd::CodeBlock) => break,
                    _ => {}
                }
            }
            Block::Code(lang, code.trim_end_matches('\n').to_string())
        }
        Tag::BlockQuote(_) => Block::Quote(inline_spans(events, None)),
        Tag::List(ordered) => {
            let mut items = Vec::new();
            loop {
                match events.next() {
                    Some(Event::Start(Tag::Item)) => {
                        let mut task = None;
                        let mut c = InlineCollector::new();
                        loop {
                            match events.next() {
                                Some(Event::TaskListMarker(b)) => task = Some(b),
                                Some(Event::End(TagEnd::Item)) => break,
                                Some(ev) => c.feed(ev),
                                None => break,
                            }
                        }
                        items.push(ListItem { task, spans: c.finish() });
                    }
                    Some(Event::End(TagEnd::List(_))) | None => break,
                    _ => {}
                }
            }
            Block::List { ordered: ordered.is_some(), items }
        }
        Tag::Table(align) => {
            let mut header = Vec::new();
            let mut rows = Vec::new();
            let mut cur_row: Option<Vec<Vec<Span>>> = None;
            let mut in_head = false;
            let mut cur_cell: Option<InlineCollector> = None;
            for ev in events {
                match ev {
                    Event::Start(Tag::TableHead) => {
                        in_head = true;
                        cur_row = Some(Vec::new());
                    }
                    Event::Start(Tag::TableRow) => cur_row = Some(Vec::new()),
                    Event::Start(Tag::TableCell) => cur_cell = Some(InlineCollector::new()),
                    Event::End(TagEnd::TableCell) => {
                        if let (Some(cell), Some(row)) = (cur_cell.take(), cur_row.as_mut()) {
                            row.push(cell.finish());
                        }
                    }
                    Event::End(TagEnd::TableRow) => {
                        if let Some(row) = cur_row.take() {
                            if in_head {
                                header = row;
                            } else {
                                rows.push(row);
                            }
                        }
                    }
                    Event::End(TagEnd::TableHead) => {
                        if let Some(row) = cur_row.take() {
                            header = row;
                        }
                        in_head = false;
                    }
                    Event::End(TagEnd::Table) => break,
                    ev if cur_cell.is_some() => cur_cell.as_mut().unwrap().feed(ev),
                    _ => {}
                }
            }
            Block::Table { align, header, rows }
        }
        Tag::HtmlBlock => Block::Raw(src.trim().to_string()),
        _ => Block::Raw(src.trim().to_string()),
    }
}

/// Collect inline spans from a stream of events. `stop` ends collection on
/// that tag end; `None` collects until the stream ends.
fn inline_spans<'a>(events: &mut impl Iterator<Item = Event<'a>>, stop: Option<TagEnd>) -> Vec<Span> {
    let mut c = InlineCollector::new();
    for ev in events {
        if let Event::End(e) = &ev {
            if Some(e) == stop.as_ref() {
                break;
            }
        }
        c.feed(ev);
    }
    c.finish()
}

struct InlineCollector {
    out: Vec<Span>,
    stack: Vec<Style>,
    link: Option<String>,
    code: Option<String>,
}

impl InlineCollector {
    fn new() -> Self {
        InlineCollector { out: Vec::new(), stack: Vec::new(), link: None, code: None }
    }

    fn flags(&self) -> Style {
        self.stack.iter().fold(Style::NONE, |a, b| a.merge(*b))
    }

    fn push(&mut self, text: String, style: Style, link: Option<String>) {
        if let Some(last) = self.out.last_mut() {
            if last.style == style && last.link == link && !text.is_empty() {
                last.text.push_str(&text);
                return;
            }
        }
        self.out.push(Span { text, style, link });
    }

    fn feed(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(Tag::Strong) => self.stack.push(Style { bold: true, ..Style::NONE }),
            Event::Start(Tag::Emphasis) => self.stack.push(Style { italic: true, ..Style::NONE }),
            Event::Start(Tag::Strikethrough) => {
                self.stack.push(Style { strike: true, ..Style::NONE })
            }
            Event::Start(Tag::Link { dest_url, .. }) => self.link = Some(dest_url.to_string()),
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.push(format!("[image: {}]", dest_url), self.flags(), None)
            }
            Event::End(TagEnd::Strong)
            | Event::End(TagEnd::Emphasis)
            | Event::End(TagEnd::Strikethrough) => {
                self.stack.pop();
            }
            Event::End(TagEnd::Link) => self.link = None,
            Event::Text(t) => {
                if let Some(b) = self.code.as_mut() {
                    b.push_str(&t);
                } else {
                    self.push(t.to_string(), self.flags(), self.link.clone());
                }
            }
            Event::Code(t) => self.push(t.to_string(), Style { code: true, ..self.flags() }, None),
            Event::SoftBreak | Event::HardBreak => {
                self.push("\n".into(), self.flags(), self.link.clone())
            }
            _ => {}
        }
    }

    fn finish(self) -> Vec<Span> {
        self.out
    }
}

// --- smart input ------------------------------------------------------------

/// Enter pressed in a block. Returns (normalized source of this block,
/// template to prefill the new next block).
pub fn on_enter(src: &str, _cursor: Option<usize>) -> (String, String) {
    let t = src.trim_end();
    let line = t.lines().last().unwrap_or("");
    let trimmed = line.trim_start();

    // Empty list item -> exit the list (drop the marker).
    if is_list_marker_only(trimmed) {
        let rest = t.lines().take(t.lines().count().saturating_sub(1)).collect::<Vec<_>>().join("\n");
        return (rest.trim_end().to_string(), String::new());
    }

    if let Some((n, sep)) = ordered_prefix(trimmed) {
        return (t.to_string(), format!("{}{} ", n + 1, sep));
    }
    if trimmed.starts_with(['-', '*', '+']) && trimmed.len() > 1 && trimmed.as_bytes()[1] == b' ' {
        return (t.to_string(), "- ".to_string());
    }
    if trimmed.starts_with('>') {
        return (t.to_string(), "> ".to_string());
    }
    (t.to_string(), String::new())
}

fn is_list_marker_only(line: &str) -> bool {
    let t = line.trim_end();
    if matches!(t, "-" | "*" | "+" | "[ ]" | "[x]" | "[X]") {
        return true;
    }
    if t.len() > 1 && (t.starts_with('-') || t.starts_with('*') || t.starts_with('+')) {
        return t[1..].trim().is_empty();
    }
    // "1." / "2)" with nothing after
    if let Some((n, sep)) = ordered_prefix_marker(t) {
        let _ = (n, sep);
        return true;
    }
    false
}

/// Like ordered_prefix but without requiring a trailing space (marker only).
fn ordered_prefix_marker(line: &str) -> Option<(u64, char)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i + 1 <= b.len() && (b[i] == b'.' || b[i] == b')') && i + 1 == b.len() {
        line[..i].parse::<u64>().ok().map(|n| (n, b[i] as char))
    } else {
        None
    }
}

fn ordered_prefix(line: &str) -> Option<(u64, char)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i + 1 < b.len() && (b[i] == b'.' || b[i] == b')') && b[i + 1] == b' ' {
        line[..i].parse::<u64>().ok().map(|n| (n, b[i] as char))
    } else {
        None
    }
}

/// Tab / Shift+Tab on a list block.
pub fn indent(src: &str, out: bool) -> String {
    src.lines()
        .map(|l| {
            if out {
                l.trim_start_matches([' ', '\t']).to_string()
            } else {
                format!("  {}", l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// True if src opens a code fence (``` or ~~~) that is not yet closed.
/// Enter inside an open fence must stay in the block, not split.
pub fn is_open_fence(src: &str) -> bool {
    let t = src.trim_start();
    let (marker, rest) = if let Some(r) = t.strip_prefix("```") {
        ("```", r)
    } else if let Some(r) = t.strip_prefix("~~~") {
        ("~~~", r)
    } else {
        return false;
    };
    // Open if the rest of the block has no line that is just the closing fence.
    !rest.lines().any(|l| l.trim() == marker)
}

/// Toggle a task checkbox in a line. Returns the new line.
pub fn toggle_task(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = if let Some(r) = t.strip_prefix("- [ ]") {
        (true, r)
    } else if let Some(r) = t.strip_prefix("- [x]") {
        (false, r)
    } else if let Some(r) = t.strip_prefix("- [X]") {
        (false, r)
    } else {
        return None;
    };
    Some(format!("- [{}]{}", if rest.0 { "x" } else { " " }, rest.1))
}

// --- document helpers --------------------------------------------------------

/// Split full markdown into blocks on blank lines.
/// ponytail: blank-line split is lossy for lists with blank lines between
/// items; upgrade to a real block splitter if that bites.
pub fn split_blocks(md: &str) -> Vec<String> {
    md.split("\n\n")
        .map(|s| s.trim_end_matches('\n').to_string())
        .filter(|s| !s.trim().is_empty())
        .collect()
}

pub fn join_blocks(blocks: &[String]) -> String {
    blocks.join("\n\n")
}

/// (words, chars, lines)
pub fn word_count(md: &str) -> (usize, usize, usize) {
    let words = md.split_whitespace().count();
    (words, md.chars().count(), md.lines().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basics() {
        assert_eq!(classify(""), Block::Empty);
        assert_eq!(classify("   \n"), Block::Empty);
        assert!(matches!(classify("# Hi"), Block::Heading(1, _)));
        assert!(matches!(classify("###### Hi"), Block::Heading(6, _)));
        assert!(matches!(classify("just text"), Block::Paragraph(_)));
        assert!(matches!(classify("> quoted"), Block::Quote(_)));
        assert!(matches!(classify("---"), Block::Hr));
        assert!(matches!(
            classify("```rust\nfn main() {}\n```"),
            Block::Code(ref l, ref c) if l == "rust" && c.contains("fn main")
        ));
    }

    #[test]
    fn inline_styles() {
        let b = classify("a **bold** and *it* and `code` and ~~strike~~");
        if let Block::Paragraph(spans) = b {
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(text, "a bold and it and code and strike");
            assert!(spans.iter().any(|s| s.style.bold && s.text == "bold"));
            assert!(spans.iter().any(|s| s.style.italic && s.text == "it"));
            assert!(spans.iter().any(|s| s.style.code && s.text == "code"));
            assert!(spans.iter().any(|s| s.style.strike && s.text == "strike"));
        } else {
            panic!("expected paragraph, got {:?}", b);
        }
    }

    #[test]
    fn nested_styles() {
        let b = classify("**bold *it* bold**");
        if let Block::Paragraph(spans) = b {
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(text, "bold it bold");
            assert!(spans.iter().all(|s| s.style.bold));
            assert!(spans.iter().any(|s| s.style.italic && s.text == "it"));
        } else {
            panic!("{:?}", b);
        }
    }

    #[test]
    fn links() {
        let b = classify("go to [site](https://x.dev) now");
        if let Block::Paragraph(spans) = b {
            assert!(spans.iter().any(|s| s.link.as_deref() == Some("https://x.dev") && s.text == "site"));
        } else {
            panic!("{:?}", b);
        }
    }

    #[test]
    fn lists_and_tasks() {
        let b = classify("- [ ] todo\n- [x] done\n- plain");
        match b {
            Block::List { ordered, items } => {
                assert!(!ordered);
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].task, Some(false));
                assert_eq!(items[1].task, Some(true));
                assert_eq!(items[2].task, None);
            }
            other => panic!("{:?}", other),
        }
        let b = classify("1. one\n2. two");
        assert!(matches!(b, Block::List { ordered: true, .. }));
    }

    #[test]
    fn table() {
        let b = classify("| a | b |\n|---|---|\n| 1 | 2 |");
        match b {
            Block::Table { header, rows, .. } => {
                assert_eq!(header.len(), 2);
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0].first().map(|s| s.text.as_str()), Some("1"));
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn enter_continues_lists() {
        assert_eq!(on_enter("- item", None).1, "- ");
        assert_eq!(on_enter("* item", None).1, "- ");
        assert_eq!(on_enter("1. one", None).1, "2. ");
        assert_eq!(on_enter("2) one", None).1, "3) ");
        assert_eq!(on_enter("> quote", None).1, "> ");
        assert_eq!(on_enter("plain", None).1, "");
        // empty list item exits
        let (src, next) = on_enter("- ", None);
        assert_eq!((src, next), (String::new(), String::new()));
        let (src, next) = on_enter("1. ", None);
        assert_eq!((src, next), (String::new(), String::new()));
    }

    #[test]
    fn enter_keeps_multiline_list() {
        let (src, next) = on_enter("- a\n- b", None);
        assert_eq!(src, "- a\n- b");
        assert_eq!(next, "- ");
    }

    #[test]
    fn toggle_task_line() {
        assert_eq!(toggle_task("- [ ] task").unwrap(), "- [x] task");
        assert_eq!(toggle_task("- [x] task").unwrap(), "- [ ] task");
        assert_eq!(toggle_task("- plain"), None);
    }

    #[test]
    fn indent_outdent() {
        assert_eq!(indent("- a", false), "  - a");
        assert_eq!(indent("  - a", true), "- a");
    }

    #[test]
    fn split_join_roundtrip() {
        let md = "# T\n\ntext\n\n- a\n- b";
        let blocks = split_blocks(md);
        assert_eq!(blocks, vec!["# T", "text", "- a\n- b"]);
        assert_eq!(join_blocks(&blocks), md);
    }

    #[test]
    fn counts() {
        assert_eq!(word_count("one two\nthree"), (3, 13, 2));
    }

    #[test]
    fn tilde_fences() {
        assert!(matches!(
            classify("~~~rust\nfn main() {}\n~~~"),
            Block::Code(ref l, ref c) if l == "rust" && c.contains("fn main")
        ));
    }

    #[test]
    fn open_fence_detection() {
        assert!(is_open_fence("```"));
        assert!(is_open_fence("```rust"));
        assert!(is_open_fence("```rust\ncode"));
        assert!(is_open_fence("~~~\nfoo"));
        assert!(!is_open_fence("```rust\ncode\n```"));
        assert!(!is_open_fence("~~~\ncode\n~~~"));
        assert!(!is_open_fence("plain text"));
        assert!(!is_open_fence(""));
    }
}
