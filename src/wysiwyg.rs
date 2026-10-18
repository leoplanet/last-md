//! Stage 1: source-aware markdown scanner.
//!
//! Classifies every byte of the markdown buffer into atoms: hidden markers
//! (skipped from the WYSIWYG layout) or styled visible content with block
//! chrome. This is the heart of the WYSIWYG: it knows, for each byte, whether
//! it is a marker to hide or content to show, and how to style it.
//!
//! Pure logic, no GUI — unit-tested.

use crate::md::Style;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockChrome {
    Paragraph,
    Heading(u8),
    /// `depth` is the nesting level (2 leading spaces or a tab per level).
    /// `index` is the displayed number of an ordered item; `task` is a
    /// `[ ]`/`[x]` checkbox item.
    ListBullet { ordered: bool, index: usize, depth: u8, task: Option<bool> },
    /// A blockquote line, `>` nesting depth (1 for `>`, 2 for `> >`).
    Quote(u8),
    Code,
    /// A table cell: column, header row?, alignment (0 left, 1 center, 2 right).
    Table { col: u8, header: bool, align: u8 },
    Hr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    /// A hidden markdown marker, skipped from the WYSIWYG layout.
    Hidden { range: Range<usize> },
    /// Visible content with an inline style and block chrome.
    Text { range: Range<usize>, style: Style, chrome: BlockChrome },
}

const CODE_STYLE: Style = Style { code: true, ..Style::NONE };

/// A fenced code block. Ranges are whole buffer lines, including their '\n'.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeBlock {
    /// The opening fence line (```lang).
    pub open: Range<usize>,
    /// The closing fence line.
    pub close: Range<usize>,
}

impl CodeBlock {
    /// Body bytes: from the first code line to the end of the last one (the
    /// last line's '\n' is the closing fence line's start - 1).
    pub fn body(&self) -> Range<usize> {
        self.open.end..self.close.start
    }

    /// Is a caret at `b` on one of the block's code lines?
    pub fn holds_caret(&self, b: usize) -> bool {
        self.open.end <= b && b < self.close.start
    }
}

/// Byte ranges of every buffer line (each including its trailing '\n').
fn line_ranges(buffer: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut ls = 0;
    for (i, b) in buffer.bytes().enumerate() {
        if b == b'\n' {
            out.push(ls..i + 1);
            ls = i + 1;
        }
    }
    out.push(ls..buffer.len());
    out
}

/// An opening fence: (fence char, fence length).
pub fn fence_open(content: &str) -> Option<(char, usize)> {
    let t = content.trim_start();
    let fc = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let flen = t.chars().take_while(|&c| c == fc).count();
    (flen >= 3).then_some((fc, flen))
}

fn fence_close(content: &str, fc: char, flen: usize) -> bool {
    let t = content.trim();
    t.len() >= flen && t.len() <= flen + 2 && t.chars().all(|c| c == fc)
}

/// Every *closed* fenced code block. An opening fence with no closing fence
/// is not a code block (it shows as text): otherwise typing ``` would swallow
/// the rest of the document into code with no way back out.
pub fn code_blocks(buffer: &str) -> Vec<CodeBlock> {
    let lines = line_ranges(buffer);
    let content = |r: &Range<usize>| buffer[r.clone()].trim_end_matches('\n');
    let mut out = Vec::new();
    let mut li = 0;
    while li < lines.len() {
        if let Some((fc, flen)) = fence_open(content(&lines[li])) {
            if let Some(cj) = (li + 1..lines.len()).find(|&j| fence_close(content(&lines[j]), fc, flen)) {
                out.push(CodeBlock { open: lines[li].clone(), close: lines[cj].clone() });
                li = cj + 1;
                continue;
            }
        }
        li += 1;
    }
    out
}

/// A GFM table: a header row, a delimiter row (`|---|:--:|`), body rows.
#[derive(Debug, Clone, PartialEq)]
pub struct TableBlock {
    /// Header line start .. end of the last row line (incl. its '\n').
    pub range: Range<usize>,
    /// The delimiter row line (incl. its '\n').
    pub delim: Range<usize>,
    /// Column alignments: 0 left, 1 center, 2 right.
    pub aligns: Vec<u8>,
}

/// Cell interiors of a table row, as ranges into `content` (pipes excluded).
fn split_cells(content: &str) -> Vec<Range<usize>> {
    let start = content.len() - content.trim_start().len();
    let end = content.trim_end().len();
    let b = content.as_bytes();
    let mut i = if b.get(start) == Some(&b'|') { start + 1 } else { start };
    let end = if end > i && b[end - 1] == b'|' && (end < 2 || b[end - 2] != b'\\') { end - 1 } else { end };
    let mut cells = Vec::new();
    let mut cell_start = i;
    while i < end {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b'|' {
            cells.push(cell_start..i);
            cell_start = i + 1;
        }
        i += 1;
    }
    cells.push(cell_start..end.max(cell_start));
    cells
}

/// Alignments if `content` is a table delimiter row.
fn delim_row(content: &str) -> Option<Vec<u8>> {
    if !content.contains('-') {
        return None;
    }
    split_cells(content)
        .into_iter()
        .map(|r| {
            let c = content[r].trim();
            let inner = c.trim_start_matches(':').trim_end_matches(':');
            (!inner.is_empty() && inner.chars().all(|ch| ch == '-'))
                .then_some(match (c.starts_with(':'), c.ends_with(':') && c.len() > 1) {
                    (true, true) => 1,
                    (false, true) => 2,
                    _ => 0,
                })
        })
        .collect()
}

/// Every GFM table (header + delimiter row with matching cell counts).
pub fn tables(buffer: &str) -> Vec<TableBlock> {
    let lines = line_ranges(buffer);
    let content = |r: &Range<usize>| buffer[r.clone()].trim_end_matches('\n');
    let mut out = Vec::new();
    let mut li = 0;
    while li + 1 < lines.len() {
        let head = content(&lines[li]);
        if head.contains('|') {
            if let Some(aligns) = delim_row(content(&lines[li + 1])) {
                if aligns.len() == split_cells(head).len() {
                    let mut last = li + 1;
                    while last + 1 < lines.len() && content(&lines[last + 1]).contains('|') {
                        last += 1;
                    }
                    out.push(TableBlock { range: lines[li].start..lines[last].end, delim: lines[li + 1].clone(), aligns });
                    li = last + 1;
                    continue;
                }
            }
        }
        li += 1;
    }
    out
}

/// Classify every byte of the markdown buffer into atoms.
pub fn scan(buffer: &str) -> Vec<Atom> {
    scan_with(buffer, None)
}

/// Like `scan`, but the inline spans (bold, link, code, ...) around the
/// caret show their markers, dimmed, as Typora does: the caret can then move
/// through them, and a link's url can be edited.
pub fn scan_at(buffer: &str, caret: usize) -> Vec<Atom> {
    scan_with(buffer, Some(caret))
}

fn scan_with(buffer: &str, caret: Option<usize>) -> Vec<Atom> {
    let mut out = Out::default();
    out.defs = buffer.lines().filter(|l| is_definition(l)).collect::<Vec<_>>().join("\n");
    let blocks = code_blocks(buffer);
    let tables = tables(buffer);
    // (ordered?, next number) per nesting depth of the current list.
    let mut counters: Vec<(bool, usize)> = Vec::new();
    let mut prev_list = false;
    // The open paragraph: (text start, last line's content end, last line's
    // end). Its lines are parsed together, so `*emphasis*` can span a soft
    // line break, as in markdown.
    let mut para: Option<(usize, usize, usize)> = None;
    let flush = |out: &mut Out, para: &mut Option<(usize, usize, usize)>, chrome: BlockChrome, style: Style| {
        if let Some((ts, ce, le)) = para.take() {
            push_inline(out, &buffer[ts..ce], ts, chrome, style);
            if ce < le {
                out.atoms.push(Atom::Hidden { range: ce..le });
            }
        }
    };

    for line_range in line_ranges(buffer) {
        let (ls, le) = (line_range.start, line_range.end);
        if ls == le && ls > 0 {
            // The empty "line" after a trailing '\n': nothing to classify.
            continue;
        }
        let line = &buffer[ls..le];
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content_end = ls + content.len();
        let after_list = prev_list;

        if para.is_some() {
            // Setext heading: a paragraph underlined with === (h1) or --- (h2).
            let t = content.trim();
            let underline = ['=', '-'].into_iter().find(|&c| !t.is_empty() && t.chars().all(|x| x == c));
            if let Some(c) = underline {
                let level = if c == '=' { 1 } else { 2 };
                flush(&mut out, &mut para, BlockChrome::Heading(level), Style { bold: true, ..Style::NONE });
                out.atoms.push(Atom::Hidden { range: ls..le });
                continue;
            }
            // A plain line continues the paragraph; anything else ends it.
            let in_block = blocks.iter().any(|b| b.open.start <= ls && ls < b.close.end)
                || tables.iter().any(|t| t.range.start <= ls && ls < t.range.end);
            let starts_block = t.is_empty()
                || in_block
                || content.starts_with('#')
                || strip_quote(content).is_some()
                || is_thematic_break(content)
                || list_marker(content).is_some();
            if !starts_block {
                para = para.map(|(ts, _, _)| (ts, content_end, le));
                continue;
            }
            flush(&mut out, &mut para, BlockChrome::Paragraph, Style::NONE);
        }

        // ---- code fences ----
        if let Some(block) = blocks.iter().find(|b| b.open.start <= ls && ls < b.close.end) {
            if ls == block.open.start || ls == block.close.start {
                out.atoms.push(Atom::Hidden { range: ls..le });
            } else {
                out.atoms.push(Atom::Text { range: ls..content_end, style: CODE_STYLE, chrome: BlockChrome::Code });
                if content_end < le {
                    out.atoms.push(Atom::Hidden { range: content_end..le });
                }
            }
            prev_list = false;
            continue;
        }

        // ---- block detection ----
        if content.starts_with('#') {
            let hashes = content.chars().take_while(|&c| c == '#').count();
            // A bare `#` stays literal text until its space is typed (Typora).
            if (1..=6).contains(&hashes) && content.as_bytes().get(hashes) == Some(&b' ') {
                let marker_end = ls + hashes + 1;
                out.atoms.push(Atom::Hidden { range: ls..marker_end });
                if marker_end >= content_end {
                    // Empty heading: a zero-width stop so the caret has a home.
                    let chrome = BlockChrome::Heading(hashes as u8);
                    out.atoms.push(Atom::Text { range: marker_end..marker_end, style: Style { bold: true, ..Style::NONE }, chrome });
                }
                push_inline(&mut out, &buffer[marker_end..content_end], marker_end, BlockChrome::Heading(hashes as u8), Style { bold: true, ..Style::NONE });
                if content_end < le {
                    out.atoms.push(Atom::Hidden { range: content_end..le });
                }
                continue;
            }
        }

        if let Some(mut rest) = strip_quote(content) {
            let mut depth = 1u8;
            while let Some(r) = strip_quote(rest) {
                (rest, depth) = (r, depth.saturating_add(1));
            }
            let marker_len = content.len() - rest.len();
            let chrome = BlockChrome::Quote(depth);
            out.atoms.push(Atom::Hidden { range: ls..ls + marker_len });
            if rest.is_empty() {
                // An empty `>` line: a zero-width stop keeps the quote bar going.
                out.atoms.push(Atom::Text { range: content_end..content_end, style: Style::NONE, chrome });
            }
            push_inline(&mut out, &buffer[ls + marker_len..content_end], ls + marker_len, chrome, Style::NONE);
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }

        if is_thematic_break(content) {
            // Markers hidden; a zero-width atom so the layout paints the rule.
            out.atoms.push(Atom::Hidden { range: ls..content_end });
            out.atoms.push(Atom::Text { range: content_end..content_end, style: Style::NONE, chrome: BlockChrome::Hr });
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            prev_list = false;
            continue;
        }

        if let Some(m) = list_marker(content) {
            let (marker_len, ordered, depth) = (m.len, m.ordered, m.depth as usize);
            if !prev_list {
                counters.clear();
            }
            counters.truncate(depth + 1);
            // A list (a deeper level, or a switch between bullets and
            // numbers) starts at its first item's number; later items count
            // on from it, as CommonMark renders them.
            let index = match counters.get(depth) {
                Some(&(o, next)) if o == ordered => next,
                _ => {
                    counters.resize(depth + 1, (ordered, 0));
                    m.number.max(if ordered { 0 } else { 1 })
                }
            };
            counters[depth] = (ordered, index + 1);
            prev_list = true;
            out.atoms.push(Atom::Hidden { range: ls..ls + marker_len });
            let chrome = BlockChrome::ListBullet { ordered, index, depth: m.depth, task: m.task };
            if ls + marker_len == content_end {
                // Empty item: a zero-width text atom so the bullet still renders.
                out.atoms.push(Atom::Text { range: content_end..content_end, style: Style::NONE, chrome });
            }
            push_inline(&mut out, &buffer[ls + marker_len..content_end], ls + marker_len, chrome, Style::NONE);
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }
        prev_list = false;

        if let Some(table) = tables.iter().find(|t| t.range.start <= ls && ls < t.range.end) {
            prev_list = false;
            if ls == table.delim.start {
                out.atoms.push(Atom::Hidden { range: ls..le });
                continue;
            }
            // Pipes and cell padding hidden; each cell's text is its own run.
            let header = ls == table.range.start;
            let mut pos = ls;
            for (col, cell) in split_cells(content).into_iter().enumerate() {
                let raw = &content[cell.clone()];
                let lead = raw.len() - raw.trim_start().len();
                let (ta, tb) = (ls + cell.start + lead, ls + cell.start + raw.trim_end().len().max(lead));
                // An empty cell's caret slot: one space in, so typing pads it.
                let ta = if ta == tb { (ls + cell.start + 1).min(ls + cell.end) } else { ta };
                let tb = tb.max(ta);
                if pos < ta {
                    out.atoms.push(Atom::Hidden { range: pos..ta });
                }
                let align = table.aligns.get(col).copied().unwrap_or(0);
                let chrome = BlockChrome::Table { col: col.min(255) as u8, header, align };
                let style = if header { Style { bold: true, ..Style::NONE } } else { Style::NONE };
                if ta == tb {
                    out.atoms.push(Atom::Text { range: ta..ta, style, chrome });
                } else {
                    push_inline(&mut out, &buffer[ta..tb], ta, chrome, style);
                }
                pos = tb;
            }
            if pos < content_end {
                out.atoms.push(Atom::Hidden { range: pos..content_end });
            }
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }

        // Indented code block: 4 spaces (or a tab) outside a list.
        let indent = if content.starts_with('\t') { 1 } else if content.starts_with("    ") { 4 } else { 0 };
        if indent > 0 && !after_list && !content.trim().is_empty() {
            out.atoms.push(Atom::Hidden { range: ls..ls + indent });
            out.atoms.push(Atom::Text { range: ls + indent..content_end, style: CODE_STYLE, chrome: BlockChrome::Code });
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }

        // `[label]: url` / `[^note]: text` definitions show as written, dimmed.
        if is_definition(content) {
            let lead = content.len() - content.trim_start().len();
            out.atoms.push(Atom::Hidden { range: ls..ls + lead });
            out.atoms.push(Atom::Text { range: ls + lead..content_end, style: Style { marker: true, ..Style::NONE }, chrome: BlockChrome::Paragraph });
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }

        // Up to 3 leading spaces mean nothing in markdown: hide them so the
        // paragraph lines up with the rest.
        let lead = content.len() - content.trim_start_matches(' ').len();
        let lead = if lead <= 3 || lead == content.len() { lead } else { 0 };
        if lead > 0 {
            out.atoms.push(Atom::Hidden { range: ls..ls + lead });
        }
        if lead == content.len() {
            // Blank line.
            if content_end < le {
                out.atoms.push(Atom::Hidden { range: content_end..le });
            }
            continue;
        }
        para = Some((ls + lead, content_end, le));
    }
    flush(&mut out, &mut para, BlockChrome::Paragraph, Style::NONE);

    if let Some(caret) = caret {
        for span in &out.spans {
            if span.range.start <= caret && caret <= span.range.end {
                for &i in &span.hidden {
                    if let Atom::Hidden { range } = &out.atoms[i] {
                        let style = Style { marker: true, ..Style::NONE };
                        out.atoms[i] = Atom::Text { range: range.clone(), style, chrome: span.chrome };
                    }
                }
            }
        }
    }
    let mut atoms = split_lines(buffer, out.atoms);
    merge_adjacent(&mut atoms);
    atoms
}

/// Split multi-line text runs (a paragraph parsed as a whole) back into
/// per-line runs: each '\n' becomes a hidden newline atom, and a
/// continuation line's leading spaces (up to 3) are hidden.
fn split_lines(buffer: &str, atoms: Vec<Atom>) -> Vec<Atom> {
    let mut out = Vec::with_capacity(atoms.len());
    for atom in atoms {
        let Atom::Text { range, style, chrome } = &atom else {
            out.push(atom);
            continue;
        };
        let text = &buffer[range.clone()];
        if !text.contains('\n') {
            out.push(atom);
            continue;
        }
        let mut s = range.start;
        for (i, _) in text.match_indices('\n') {
            let n = range.start + i;
            if n > s {
                out.push(Atom::Text { range: s..n, style: *style, chrome: *chrome });
            }
            out.push(Atom::Hidden { range: n..n + 1 });
            let lead = buffer[n + 1..range.end].bytes().take(3).take_while(|&b| b == b' ').count();
            if lead > 0 {
                out.push(Atom::Hidden { range: n + 1..n + 1 + lead });
            }
            s = n + 1 + lead;
        }
        if s < range.end {
            out.push(Atom::Text { range: s..range.end, style: *style, chrome: *chrome });
        }
    }
    out
}

/// Scanner output: atoms plus the inline spans whose markers can be revealed.
#[derive(Default)]
struct Out {
    atoms: Vec<Atom>,
    spans: Vec<InlineSpan>,
    /// The document's `[label]: url` and `[^note]:` definition lines.
    defs: String,
}

/// An inline span (marker start .. marker end) and its hidden marker atoms.
struct InlineSpan {
    range: Range<usize>,
    hidden: Vec<usize>,
    chrome: BlockChrome,
}

impl Out {
    fn hide(&mut self, range: Range<usize>) -> usize {
        self.atoms.push(Atom::Hidden { range });
        self.atoms.len() - 1
    }
}


/// Push `text` (at buffer offset `base`) as atoms: what CommonMark shows is
/// visible, styled; every marker byte is hidden. Inline parsing is done by
/// pulldown-cmark, so emphasis nesting, flanking rules, escapes, code spans
/// and reference links render exactly as the spec (and Typora) says.
fn push_inline(out: &mut Out, text: &str, base: usize, chrome: BlockChrome, base_style: Style) {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    if text.is_empty() {
        return;
    }
    // `)` can't start a block, and for emphasis flanking it behaves like the
    // start of a line, so the text parses as one plain paragraph. The
    // document's link/footnote definitions follow, so references resolve.
    let src = format!("){text}\n\n{}", out.defs);
    let end = 1 + text.len();
    let local = |r: Range<usize>| r.start.clamp(1, end) - 1..r.end.clamp(1, end) - 1;

    // Visible runs and inline spans (marker reveal), relative to `text`.
    let mut visible: Vec<(Range<usize>, Style)> = Vec::new();
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut styles = vec![base_style];
    // Open inline HTML formatting tags: (name, style depth, tag start).
    let mut html: Vec<(String, usize, usize)> = Vec::new();
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES;
    for (ev, r) in Parser::new_ext(&src, opts).into_offset_iter() {
        if r.start >= end {
            break;
        }
        let r = local(r);
        let top = *styles.last().unwrap();
        match ev {
            Event::Start(tag) => {
                let style = match tag {
                    Tag::Emphasis => Style { italic: true, ..Style::NONE },
                    Tag::Strong => Style { bold: true, ..Style::NONE },
                    Tag::Strikethrough => Style { strike: true, ..Style::NONE },
                    Tag::Link { .. } => Style { link: true, ..Style::NONE },
                    Tag::Image { .. } => Style { italic: true, ..Style::NONE },
                    _ => continue,
                };
                styles.push(top.merge(style));
                spans.push(r);
            }
            Event::End(pulldown_cmark::TagEnd::Paragraph) => {}
            Event::End(_) => {
                if styles.len() > 1 {
                    styles.pop();
                }
            }
            Event::Text(_) => push_text(&mut visible, text, r, top),
            Event::Code(_) => {
                // Inner text, minus the backtick runs and one padding space a side.
                let raw = &text[r.clone()];
                let n = raw.bytes().take_while(|&b| b == b'`').count();
                let (mut a, mut b) = (r.start + n, r.end - n);
                let inner = &text[a..b];
                if inner.len() >= 2 && inner.starts_with([' ', '\n']) && inner.ends_with([' ', '\n']) && !inner.trim().is_empty() {
                    (a, b) = (a + 1, b - 1);
                }
                visible.push((a..b, top.merge(CODE_STYLE)));
                spans.push(r);
            }
            Event::FootnoteReference(_) => {
                // [^1] shows its label, link-coloured.
                visible.push((r.start + 2..r.end - 1, top.merge(Style { link: true, ..Style::NONE })));
                spans.push(r);
            }
            Event::InlineHtml(tag) => {
                let tag = tag.trim().to_ascii_lowercase();
                if let Some(name) = tag.strip_prefix("</").and_then(|t| t.strip_suffix('>')) {
                    if let Some(pos) = html.iter().rposition(|(n, _, _)| n == name) {
                        let (_, depth, start) = html[pos].clone();
                        html.truncate(pos);
                        styles.truncate(depth);
                        spans.push(start..r.end);
                        continue;
                    }
                } else if let Some(name) = tag.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
                    let closed = text[r.end..].to_ascii_lowercase().contains(&format!("</{name}>"));
                    if let (Some(style), true) = (html_style(name), closed) {
                        html.push((name.to_string(), styles.len(), r.start));
                        styles.push(top.merge(style));
                        continue;
                    }
                }
                // Any other tag stays as written.
                visible.push((r, top));
            }
            // Breaks, and anything else, are markup: hidden.
            _ => {}
        }
    }

    // Lay the runs out over the text: gaps between visible runs are hidden.
    let mut hidden: Vec<(usize, Range<usize>)> = Vec::new();
    let mut hide = |out: &mut Out, r: Range<usize>| {
        // Each '\n' starts its own hidden atom (the layout's line breaks).
        let mut s = r.start;
        for (i, _) in text[r.clone()].match_indices('\n') {
            let n = r.start + i;
            if n > s {
                hidden.push((out.hide(base + s..base + n), s..n));
            }
            s = n;
            if n + 1 == r.end || text.as_bytes().get(n + 1) != Some(&b' ') {
                out.hide(base + n..base + n + 1);
                s = n + 1;
            }
        }
        if s < r.end {
            let i = out.hide(base + s..base + r.end);
            if !text[s..r.end].starts_with('\n') {
                hidden.push((i, s..r.end));
            }
        }
    };
    let mut pos = 0;
    for (r, style) in visible {
        if r.start < pos || r.is_empty() {
            continue;
        }
        if pos < r.start {
            hide(out, pos..r.start);
        }
        out.atoms.push(Atom::Text { range: base + r.start..base + r.end, style, chrome });
        pos = r.end;
    }
    if pos < text.len() {
        hide(out, pos..text.len());
    }
    // Spans reveal the markers inside them; a marker in no span (an escape's
    // backslash, a hard break) reveals on its own.
    for span in &spans {
        let ids = hidden.iter().filter(|(_, h)| span.start <= h.start && h.end <= span.end).map(|(i, _)| *i).collect();
        out.spans.push(InlineSpan { range: base + span.start..base + span.end, hidden: ids, chrome });
    }
    for (i, h) in &hidden {
        if !spans.iter().any(|s| s.start <= h.start && h.end <= s.end) {
            out.spans.push(InlineSpan { range: base + h.start..base + h.end, hidden: vec![*i], chrome });
        }
    }
}

/// Push a visible text run, styling bare `http(s)://` / `www.` URLs as links.
fn push_text(visible: &mut Vec<(Range<usize>, Style)>, text: &str, r: Range<usize>, style: Style) {
    if style.link || style.code {
        visible.push((r, style));
        return;
    }
    let s = &text[r.clone()];
    let mut pos = 0;
    while let Some(found) = ["https://", "http://", "www."].iter().filter_map(|p| s[pos..].find(p).map(|i| pos + i)).min() {
        let at_word_start = found == 0 || !s[..found].ends_with(|c: char| c.is_alphanumeric());
        let len = s[found..].find(|c: char| c.is_whitespace() || c == '<').unwrap_or(s.len() - found);
        // Trailing punctuation ends a sentence, not the URL.
        let len = s[found..found + len].trim_end_matches(['.', ',', ':', ';', '!', '?', ')', '"', '\'']).len();
        if !at_word_start || len <= 8 {
            pos = found + 1;
            continue;
        }
        if found > 0 {
            visible.push((r.start..r.start + found, style));
        }
        visible.push((r.start + found..r.start + found + len, style.merge(Style { link: true, ..Style::NONE })));
        return push_text(visible, text, r.start + found + len..r.end, style);
    }
    visible.push((r, style));
}

/// The style of an inline HTML formatting tag.
fn html_style(name: &str) -> Option<Style> {
    Some(match name {
        "b" | "strong" => Style { bold: true, ..Style::NONE },
        "i" | "em" => Style { italic: true, ..Style::NONE },
        "u" | "ins" => Style { underline: true, ..Style::NONE },
        "s" | "del" | "strike" => Style { strike: true, ..Style::NONE },
        "code" | "kbd" => CODE_STYLE,
        _ => return None,
    })
}

/// A `[label]: url` link reference or `[^note]: text` footnote definition.
fn is_definition(line: &str) -> bool {
    let t = line.trim_start_matches(' ');
    line.len() - t.len() <= 3 && t.starts_with('[') && t.find("]:").is_some_and(|i| i > 1 && !t[..i].contains(']'))
}

fn strip_quote(content: &str) -> Option<&str> {
    let t = content.trim_start_matches(' ');
    if content.len() - t.len() <= 3 && t.starts_with('>') {
        let after = &t[1..];
        Some(after.strip_prefix(' ').unwrap_or(after))
    } else {
        None
    }
}

fn is_thematic_break(content: &str) -> bool {
    let t: String = content.trim().chars().filter(|c| *c != ' ' && *c != '\t').collect();
    content.len() - content.trim_start().len() <= 3
        && t.len() >= 3
        && t.chars().all(|c| c == '-' || c == '*' || c == '_')
        && t.chars().collect::<std::collections::HashSet<char>>().len() == 1
}

struct ListMarker {
    /// Marker bytes to hide: indentation, bullet/number, space, task box.
    len: usize,
    ordered: bool,
    depth: u8,
    number: usize,
    task: Option<bool>,
}

/// Detect a list marker (after optional indentation) at the start.
fn list_marker(content: &str) -> Option<ListMarker> {
    let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
    let depth = content[..indent].chars().map(|c| if c == '\t' { 2 } else { 1 }).sum::<usize>() / 2;
    let depth = depth.min(u8::MAX as usize) as u8;
    let b = &content.as_bytes()[indent..];
    let (len, ordered, number) = if b.len() > 1 && (b[0] == b'-' || b[0] == b'*' || b[0] == b'+') && b[1] == b' ' {
        (indent + 2, false, 0)
    } else {
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if !(i > 0 && i < 10 && i + 1 < b.len() && (b[i] == b'.' || b[i] == b')') && b[i + 1] == b' ') {
            return None;
        }
        (indent + i + 2, true, content[indent..indent + i].parse().unwrap_or(1))
    };
    // Task item: "[ ] " / "[x] " (or a bare box at the end of the line).
    let after = &content[len..];
    let task = ["[ ]", "[x]", "[X]"]
        .iter()
        .find(|t| after.starts_with(*t) && (after.len() == 3 || after.as_bytes()[3] == b' '))
        .map(|t| *t != "[ ]");
    let len = len + if task.is_some() { after.len().min(4) } else { 0 };
    Some(ListMarker { len, ordered, depth, number, task })
}

/// Merge adjacent Text atoms with the same style and chrome.
fn merge_adjacent(atoms: &mut Vec<Atom>) {
    let mut out: Vec<Atom> = Vec::with_capacity(atoms.len());
    for atom in atoms.drain(..) {
        if let Some(last) = out.last_mut() {
            if let (Atom::Text { range: lr, style: ls, chrome: lc },
                    Atom::Text { range: ar, style: as_, chrome: ac }) =
                (last, &atom)
            {
                if *ls == *as_ && *lc == *ac && lr.end == ar.start && lr.start != lr.end && ar.start != ar.end {
                    lr.end = ar.end;
                    continue;
                }
            }
        }
        out.push(atom);
    }
    *atoms = out;
}
