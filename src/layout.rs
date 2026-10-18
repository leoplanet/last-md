//! Stage 3: WYSIWYG layout. Paint the buffer as Typora-style WYSIWYG:
//! markers hidden, block chrome rendered (heading size, list bullet, quote
//! bar, code box, rule), inline styles applied, selection + caret drawn.
//!
//! Each line is laid out as a single `LayoutJob` of its visible text, so the
//! caret and selection sit at *exact* glyph positions via
//! `Galley::pos_from_cursor`.
//!
//! Block chrome never lives in the galley text. A line's content starts at a
//! fixed indent for its block (list depth, quote), and the bullet / number /
//! checkbox is painted in the gutter left of it. So the content column never
//! depends on marker width ("9." vs "10.", bullet vs number), and wrapped
//! rows hang under the text instead of returning to the left edge.
//!
//! Caret mapping is per line and direct: each `LinePaint` records the buffer
//! byte of every visible char it shows, so a buffer byte maps to a galley char
//! index (and back) without a global rendered index. A global index can't
//! tell "end of line N" from "start of line N+1" — both sit on the hidden
//! newline — which put the caret on the wrong line.

use std::sync::Arc;

use eframe::egui::{Align2, Color32, FontFamily, Pos2, Rect, Ui, Vec2};
use eframe::egui::epaint::text::cursor::CCursor;
use eframe::egui::epaint::text::{FontId, Galley, LayoutJob, TextFormat};
use eframe::egui::epaint::Stroke;

use crate::md::Style;
use crate::wysiwyg::{Atom, BlockChrome};

pub const BOLD_FAMILY: &str = "InterBold";
const BASE: f32 = 18.0;
const CODE_PAD: f32 = 6.0;
/// Content indent per list level (Typora: 30px at 16px ≈ 1.9em).
pub const LIST_INDENT: f32 = 32.0;
/// Content indent of a block quote (bar + gap).
pub const QUOTE_INDENT: f32 = 18.0;
/// Content indent inside a code box.
pub const CODE_INDENT: f32 = 10.0;

/// One laid-out line: its galley (for exact glyph positions), screen origin,
/// and the buffer bytes it shows.
#[derive(Clone)]
pub struct LinePaint {
    pub galley: Arc<Galley>,
    /// Screen position of the galley (the content column, after any indent).
    pub origin: Pos2,
    pub height: f32,
    /// First buffer byte of the line (including hidden leading markers).
    pub buf_start: usize,
    /// Line-end caret position: the byte of the trailing '\n', or the end of
    /// the buffer on the last line.
    pub buf_end: usize,
    /// Buffer byte of each visible char, in galley order.
    pub chars: Vec<usize>,
    /// Screen rect of a task checkbox, for click-to-toggle.
    pub checkbox: Option<Rect>,
}

impl LinePaint {
    /// Is buffer byte `b` a caret position on this line?
    pub fn contains(&self, b: usize) -> bool {
        self.buf_start <= b && b <= self.buf_end
    }

    /// Galley char index for a caret at buffer byte `b`.
    pub fn galley_index(&self, b: usize) -> usize {
        self.chars.partition_point(|&c| c < b)
    }

    /// Buffer byte for a caret at galley char index `g`.
    pub fn byte_at(&self, g: usize) -> usize {
        self.chars.get(g).copied().unwrap_or(self.buf_end)
    }

    /// Screen rect (zero width, row height) of a caret at buffer byte `b`.
    pub fn caret_rect(&self, b: usize) -> Rect {
        let r = self.galley.pos_from_cursor(CCursor::new(self.galley_index(b)));
        r.translate(self.origin.to_vec2())
    }

    /// Buffer byte for a screen position over this line (clicks in the
    /// gutter land on the first char).
    pub fn byte_at_pos(&self, pos: Pos2) -> usize {
        self.byte_at(self.galley.cursor_from_pos(pos - self.origin).index.0)
    }
}

/// The line that shows the caret at buffer byte `b`. A caret inside hidden
/// lines (e.g. a closing fence at the end) falls back to the nearest line
/// before it.
pub fn line_for_byte(lines: &[LinePaint], b: usize) -> Option<&LinePaint> {
    lines
        .iter()
        .find(|l| l.contains(b))
        .or_else(|| lines.iter().rev().find(|l| l.buf_start <= b))
        .or_else(|| lines.first())
}

/// Screen rect of the caret at buffer byte `b`.
pub fn caret_rect(lines: &[LinePaint], b: usize) -> Option<Rect> {
    line_for_byte(lines, b).map(|l| l.caret_rect(b.clamp(l.buf_start, l.buf_end)))
}

/// Buffer byte under a screen position: the line whose vertical band holds
/// `pos` (else the nearest band), then the glyph within it.
pub fn byte_at_pos(lines: &[LinePaint], pos: Pos2) -> Option<usize> {
    let line = lines.iter().min_by(|a, b| band_dist(a, pos.y).total_cmp(&band_dist(b, pos.y)))?;
    Some(line.byte_at_pos(pos))
}

fn band_dist(line: &LinePaint, y: f32) -> f32 {
    let (top, bottom) = (line.origin.y, line.origin.y + line.height);
    if y < top { top - y } else if y > bottom { y - bottom } else { 0.0 }
}

/// Heading sizes as in Typora's default (GitHub) theme: 2.25/1.75/1.5/1.25/1/1 em.
fn heading_size(level: u8) -> f32 {
    BASE * match level {
        1 => 2.25,
        2 => 1.75,
        3 => 1.5,
        4 => 1.25,
        _ => 1.0,
    }
}

/// The block chrome of a line (its first text atom's).
fn line_chrome(line: &LineGroup, atoms: &[Atom]) -> Option<BlockChrome> {
    atoms[line.atoms.clone()].iter().find_map(|a| match a {
        Atom::Text { chrome, .. } => Some(*chrome),
        _ => None,
    })
}

fn weak_color(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(150, 150, 160) } else { Color32::from_rgb(120, 120, 130) }
}

/// The caret is an accent color, not the text color: at the start of a line
/// a text-colored 1.5px bar merges into the first glyph's stem.
fn caret_color(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(110, 170, 255) } else { Color32::from_rgb(20, 100, 230) }
}

/// Link color (Typora GitHub theme #4183C4, lifted for dark mode).
fn link_color(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(110, 165, 230) } else { Color32::from_rgb(65, 131, 196) }
}

fn text_color(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(232, 232, 236) } else { Color32::from_rgb(28, 28, 32) }
}

fn rule_color(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(60, 60, 68) } else { Color32::from_rgb(230, 230, 234) }
}

struct BlockGroup {
    lines: Vec<LineGroup>,
    kind: BlockKind,
}

#[derive(PartialEq)]
enum BlockKind {
    Code,
    List,
    Quote,
    Table,
    Single,
}

/// Does this setext heading line go on to the next line (the underline is
/// still to come)? ATX (`#`) headings are always a single line.
fn heading_continues(buffer: &str, line: &LineGroup) -> bool {
    let start = buffer[..line.buf_start].rfind('\n').map_or(0, |i| i + 1);
    if buffer[start..].trim_start().starts_with('#') || line.buf_end >= buffer.len() {
        return false;
    }
    let next = buffer[line.buf_end + 1..].lines().next().unwrap_or("").trim();
    let underline = !next.is_empty() && (next.chars().all(|c| c == '=') || next.chars().all(|c| c == '-'));
    !underline && !next.is_empty()
}

/// Space (and the h1/h2 rule) after a heading's last line.
fn heading_end(ui: &mut Ui, chrome: Option<BlockChrome>, dark: bool) {
    if let Some(BlockChrome::Heading(level)) = chrome {
        if level <= 2 {
            ui.add_space(BASE * 0.25);
            let (y, x) = (ui.cursor().min.y, ui.cursor().min.x);
            ui.painter().hline(x..=x + ui.available_width(), y, Stroke::new(1.0, rule_color(dark)));
            ui.add_space(1.0);
        }
        ui.add_space(BASE * 0.4);
    }
}

/// Horizontal padding inside a table cell.
pub const CELL_PAD: f32 = 10.0;
/// Vertical padding inside a table row.
const ROW_PAD: f32 = 5.0;

/// Paint the WYSIWYG document into `ui`, returning the laid-out lines (for
/// caret/selection/mouse mapping) and the caret's screen rect.
pub fn paint(
    ui: &mut Ui,
    buffer: &str,
    atoms: &[Atom],
    caret: usize,
    caret_on: bool,
    selection: Option<(usize, usize)>,
) -> (Vec<LinePaint>, Option<Rect>) {
    let dark = ui.visuals().dark_mode;
    let lines = group_lines(buffer, atoms);
    let blocks = group_blocks(lines, atoms);
    let mut out: Vec<LinePaint> = Vec::new();

    // Inside a multi-line setext heading (its next line is more heading).
    let mut heading_open = false;
    for block in &blocks {
        if block.kind != BlockKind::Single {
            heading_open = false;
        }
        match block.kind {
            BlockKind::Code => paint_code_block(ui, buffer, atoms, block, dark, &mut out),
            BlockKind::Table => paint_table(ui, buffer, atoms, block, dark, &mut out),
            BlockKind::Quote => {
                let bar = if dark { Color32::from_rgb(90, 90, 110) } else { Color32::from_rgb(200, 200, 210) };
                for (n, line) in block.lines.iter().enumerate() {
                    let depth = match line_chrome(line, atoms) {
                        Some(BlockChrome::Quote(d)) => d.max(1),
                        _ => 1,
                    };
                    let top = ui.cursor().min;
                    paint_line(ui, buffer, atoms, line, dark, QUOTE_INDENT * depth as f32, &mut out);
                    if n + 1 < block.lines.len() {
                        ui.add_space(2.0);
                    }
                    // A bar per nesting level, joined to the next line's.
                    let bottom = ui.cursor().min.y;
                    for k in 0..depth {
                        let x = top.x + QUOTE_INDENT * k as f32;
                        ui.painter().rect_filled(Rect::from_min_max(Pos2::new(x, top.y), Pos2::new(x + 3.0, bottom)), 0.0, bar);
                    }
                }
                ui.add_space(2.0);
            }
            BlockKind::List => {
                for line in &block.lines {
                    let depth = match line_chrome(line, atoms) {
                        Some(BlockChrome::ListBullet { depth, .. }) => depth as f32,
                        _ => 0.0,
                    };
                    paint_line(ui, buffer, atoms, line, dark, LIST_INDENT * (depth + 1.0), &mut out);
                    paint_list_marker(ui, line, atoms, dark, out.last_mut().unwrap());
                    ui.add_space(2.0);
                }
            }
            BlockKind::Single => {
                for line in &block.lines {
                    let continues = heading_continues(buffer, line);
                    let continued = std::mem::replace(&mut heading_open, continues);
                    match line_chrome(line, atoms) {
                        Some(BlockChrome::Heading(_)) if continues || continued => {
                            // A line of a multi-line (setext) heading.
                            if !continued && !out.is_empty() {
                                ui.add_space(BASE * 0.6);
                            }
                            paint_line(ui, buffer, atoms, line, dark, 0.0, &mut out);
                            if !continues {
                                heading_end(ui, line_chrome(line, atoms), dark);
                            }
                        }
                        Some(BlockChrome::Heading(level)) => {
                            if !out.is_empty() {
                                // Typora: 1rem above headings.
                                ui.add_space(BASE * 0.6);
                            }
                            paint_line(ui, buffer, atoms, line, dark, 0.0, &mut out);
                            if level <= 2 {
                                // h1/h2 rule underneath.
                                ui.add_space(BASE * 0.25);
                                let (y, x) = (ui.cursor().min.y, ui.cursor().min.x);
                                ui.painter().hline(x..=x + ui.available_width(), y, Stroke::new(1.0, rule_color(dark)));
                                ui.add_space(1.0);
                            }
                            ui.add_space(BASE * 0.4);
                        }
                        Some(BlockChrome::Hr) => {
                            paint_line(ui, buffer, atoms, line, dark, 0.0, &mut out);
                            let l = out.last().unwrap();
                            let y = l.origin.y + l.height / 2.0;
                            ui.painter().hline(l.origin.x..=l.origin.x + ui.available_width(), y, Stroke::new(2.0, rule_color(dark)));
                        }
                        _ => paint_line(ui, buffer, atoms, line, dark, 0.0, &mut out),
                    }
                    ui.add_space(2.0);
                }
            }
        }
    }

    // An empty doc, or an empty trailing line (buffer ends with '\n'), has no
    // atoms of its own — paint an empty line so the caret can sit there.
    if buffer.is_empty() || buffer.ends_with('\n') {
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(empty_job()));
        let (_, rect) = ui.allocate_space(Vec2::new(1.0, BASE));
        out.push(LinePaint {
            galley,
            origin: rect.min,
            height: BASE,
            buf_start: buffer.len(),
            buf_end: buffer.len(),
            chars: Vec::new(),
            checkbox: None,
        });
    }

    paint_selection(ui, &out, selection, dark);
    let caret_rect = caret_rect(&out, caret).map(|r| Rect::from_min_size(r.min - Vec2::new(1.0, 0.0), Vec2::new(2.0, r.height())));
    if let (Some(r), true) = (caret_rect, caret_on) {
        ui.painter().rect_filled(r, 0.0, caret_color(dark));
    }
    (out, caret_rect)
}

/// Paint a list item's bullet, number or checkbox in the gutter left of its
/// content, centered on the first row.
fn paint_list_marker(ui: &Ui, line: &LineGroup, atoms: &[Atom], dark: bool, lp: &mut LinePaint) {
    let Some(BlockChrome::ListBullet { ordered, index, depth, task }) = line_chrome(line, atoms) else { return };
    let row_mid = lp.origin.y + lp.galley.rows.first().map_or(BASE / 2.0, |r| r.rect().center().y);
    let gap = 8.0;
    let right = lp.origin.x - gap;
    let painter = ui.painter();
    if let Some(checked) = task {
        let r = Rect::from_center_size(Pos2::new(right - 6.0, row_mid), Vec2::splat(13.0));
        let stroke = Stroke::new(1.2, weak_color(dark));
        if checked {
            painter.rect_filled(r, 2.5, caret_color(dark));
            let ink = if dark { Color32::from_rgb(20, 20, 24) } else { Color32::WHITE };
            let pts = [r.left_center() + Vec2::new(3.0, 0.0), r.center_bottom() + Vec2::new(-1.0, -3.5), r.right_top() + Vec2::new(-3.0, 3.5)];
            painter.line_segment([pts[0], pts[1]], Stroke::new(1.8, ink));
            painter.line_segment([pts[1], pts[2]], Stroke::new(1.8, ink));
        } else {
            painter.rect_stroke(r, 2.5, stroke, eframe::egui::StrokeKind::Inside);
        }
        lp.checkbox = Some(r.expand(3.0));
    } else if ordered {
        painter.text(Pos2::new(right, row_mid), Align2::RIGHT_CENTER, format!("{index}."), FontId::new(BASE, FontFamily::Proportional), weak_color(dark));
    } else {
        // Typora cycles disc / circle / square by depth.
        let c = Pos2::new(right - 5.0, row_mid);
        match depth % 3 {
            0 => { painter.circle_filled(c, 2.8, weak_color(dark)); }
            1 => { painter.circle_stroke(c, 2.8, Stroke::new(1.2, weak_color(dark))); }
            _ => { painter.rect_filled(Rect::from_center_size(c, Vec2::splat(5.0)), 0.0, weak_color(dark)); }
        }
    }
}

/// A job with no text still needs the body font so the empty row has the
/// right height for the caret.
fn empty_job() -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append("", 0.0, TextFormat::simple(FontId::new(BASE, FontFamily::Proportional), Color32::TRANSPARENT));
    job
}

/// Build the `LayoutJob` for one line's visible text, with the buffer byte of
/// each visible char.
fn build_line_job(
    buffer: &str,
    atoms: &[Atom],
    line: &LineGroup,
    dark: bool,
    in_code: bool,
    wrap_width: f32,
) -> (LayoutJob, Vec<usize>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.first_row_min_height = BASE;

    let mut chars = Vec::new();
    let mut empty_format = None;
    for a in &atoms[line.atoms.clone()] {
        if let Atom::Text { range: r, style, chrome } = a {
            if r.is_empty() {
                empty_format.get_or_insert_with(|| text_format(*style, *chrome, dark, in_code));
                continue;
            }
            let text = &buffer[r.clone()];
            chars.extend(text.char_indices().map(|(i, _)| r.start + i));
            job.append(text, 0.0, text_format(*style, *chrome, dark, in_code));
        }
    }
    if job.text.is_empty() {
        job = empty_job();
        if let Some(format) = empty_format {
            // Keep an empty heading as tall as it will be once typed in.
            job.sections[0].format = format;
            job.first_row_min_height = BASE;
        }
    }
    (job, chars)
}

/// The text format of a visible run.
fn text_format(style: Style, chrome: BlockChrome, dark: bool, in_code: bool) -> TextFormat {
    let text_color = text_color(dark);
    let weak_color = weak_color(dark);
    let (size, family, color) = if in_code {
        (BASE - 2.0, FontFamily::Monospace, text_color)
    } else {
        match chrome {
            BlockChrome::Heading(level) => (
                heading_size(level),
                FontFamily::Name(BOLD_FAMILY.into()),
                if level == 6 { weak_color } else { text_color },
            ),
            BlockChrome::Code => (BASE - 2.0, FontFamily::Monospace, text_color),
            _ => (BASE, FontFamily::Proportional, if style.code { weak_color } else { text_color }),
        }
    };
    let color = if style.link && !in_code { link_color(dark) } else { color };
    if style.marker {
        // A revealed markdown marker: dimmed, in the plain body font.
        let size = if let BlockChrome::Heading(l) = chrome { heading_size(l) } else { size };
        return TextFormat::simple(FontId::new(size, if in_code { FontFamily::Monospace } else { FontFamily::Proportional }), weak_color);
    }
    let mut fmt = TextFormat {
        font_id: FontId::new(size, family),
        color,
        italics: style.italic,
        ..Default::default()
    };
    if style.bold && !in_code && !matches!(chrome, BlockChrome::Heading(_)) {
        fmt.font_id = FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()));
    }
    if style.code && !in_code {
        // Monospace runs ~10% larger than Inter at the same size.
        fmt.font_id = FontId::new(size * 0.9, FontFamily::Monospace);
        fmt.background = if dark { Color32::from_rgb(44, 44, 52) } else { Color32::from_rgb(240, 240, 244) };
        fmt.valign = eframe::egui::Align::Center;
    }
    if style.strike {
        fmt.strikethrough = Stroke::new(1.0, color);
    }
    if style.underline {
        fmt.underline = Stroke::new(1.0, color);
    }
    fmt
}

/// Paint a table as a grid. Each row is still one galley (so caret mapping
/// is unchanged); cells are pushed to their column with section
/// `leading_space`, column widths are the widest cell.
fn paint_table(ui: &mut Ui, buffer: &str, atoms: &[Atom], block: &BlockGroup, dark: bool, out: &mut Vec<LinePaint>) {
    // Per row, per cell: (col, align, [(range, style, chrome)]).
    type Cell = (usize, u8, Vec<(std::ops::Range<usize>, Style, BlockChrome)>);
    let rows: Vec<Vec<Cell>> = block
        .lines
        .iter()
        .map(|line| {
            let mut cells: Vec<Cell> = Vec::new();
            for a in &atoms[line.atoms.clone()] {
                if let Atom::Text { range, style, chrome: chrome @ BlockChrome::Table { col, align, .. } } = a {
                    match cells.last_mut() {
                        Some(c) if c.0 == *col as usize => c.2.push((range.clone(), *style, *chrome)),
                        _ => cells.push((*col as usize, *align, vec![(range.clone(), *style, *chrome)])),
                    }
                }
            }
            cells
        })
        .collect();
    // An empty cell shows a placeholder space so it has a caret slot.
    let cell_job = |cell: &Cell| {
        let mut job = LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        for (r, style, chrome) in &cell.2 {
            if !r.is_empty() {
                job.append(&buffer[r.clone()], 0.0, text_format(*style, *chrome, dark, false));
            }
        }
        if job.text.is_empty() {
            job.append(" ", 0.0, text_format(Style::NONE, cell.2[0].2, dark, false));
        }
        job
    };
    let widths: Vec<Vec<f32>> = rows
        .iter()
        .map(|cells| cells.iter().map(|c| ui.fonts_mut(|f| f.layout_job(cell_job(c))).size().x).collect())
        .collect();
    let ncols = rows.iter().flat_map(|r| r.iter().map(|c| c.0 + 1)).max().unwrap_or(0);
    let mut col_w = vec![24.0f32; ncols];
    for (cells, ws) in rows.iter().zip(&widths) {
        for (c, w) in cells.iter().zip(ws) {
            col_w[c.0] = col_w[c.0].max(*w);
        }
    }
    let mut col_x = vec![0.0f32; ncols + 1];
    for c in 0..ncols {
        col_x[c + 1] = col_x[c] + col_w[c] + 2.0 * CELL_PAD;
    }

    let grid = rule_color(dark);
    let header_bg = if dark { Color32::from_rgb(36, 36, 42) } else { Color32::from_rgb(246, 246, 248) };
    let top = ui.cursor().min;
    let mut row_ys = vec![top.y];
    for ((line, cells), ws) in block.lines.iter().zip(&rows).zip(&widths) {
        let mut job = LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        job.first_row_min_height = BASE;
        let mut chars = Vec::new();
        let mut x = 0.0;
        for (cell, w) in cells.iter().zip(ws) {
            let (col, align) = (cell.0, cell.1);
            let slack = col_w[col] - w;
            let target = col_x[col] + CELL_PAD + match align {
                1 => slack / 2.0,
                2 => slack,
                _ => 0.0,
            };
            let mut lead = target - x;
            let empty = cell.2.iter().all(|(r, ..)| r.is_empty());
            for (r, style, chrome) in &cell.2 {
                if empty {
                    job.append(" ", lead, text_format(*style, *chrome, dark, false));
                    chars.push(r.start);
                    break;
                }
                if r.is_empty() {
                    continue;
                }
                job.append(&buffer[r.clone()], lead, text_format(*style, *chrome, dark, false));
                chars.extend(buffer[r.clone()].char_indices().map(|(i, _)| r.start + i));
                lead = 0.0;
            }
            x = target + w;
        }
        let header = matches!(cells.first().and_then(|c| c.2.first()), Some((_, _, BlockChrome::Table { header: true, .. })));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let h = galley.size().y.max(BASE) + 2.0 * ROW_PAD;
        let (_, rect) = ui.allocate_space(Vec2::new(col_x[ncols], h));
        if header {
            ui.painter().rect_filled(Rect::from_min_size(rect.min, Vec2::new(col_x[ncols], h)), 0.0, header_bg);
        }
        let origin = rect.min + Vec2::new(0.0, ROW_PAD);
        ui.painter().galley(origin, galley.clone(), text_color(dark));
        out.push(LinePaint {
            galley,
            origin,
            height: h - 2.0 * ROW_PAD,
            buf_start: line.buf_start,
            buf_end: line.buf_end,
            chars,
            checkbox: None,
        });
        row_ys.push(rect.max.y);
    }
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, grid);
    for y in &row_ys {
        painter.hline(top.x..=top.x + col_x[ncols], *y, stroke);
    }
    for x in &col_x {
        painter.vline(top.x + x, top.y..=*row_ys.last().unwrap(), stroke);
    }
    ui.add_space(4.0);
}

/// Lay out one line with its content at `indent`, allocate + paint it, and
/// record it.
fn paint_line(
    ui: &mut Ui,
    buffer: &str,
    atoms: &[Atom],
    line: &LineGroup,
    dark: bool,
    indent: f32,
    out: &mut Vec<LinePaint>,
) {
    let wrap = (ui.available_width() - indent).max(40.0);
    let (job, chars) = build_line_job(buffer, atoms, line, dark, false, wrap);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let size = galley.size();
    let (_, rect) = ui.allocate_space(Vec2::new(indent + size.x.max(1.0), size.y.max(BASE)));
    let origin = rect.min + Vec2::new(indent, 0.0);
    ui.painter().galley(origin, galley.clone(), text_color(dark));
    out.push(LinePaint {
        galley,
        origin,
        height: size.y.max(BASE),
        buf_start: line.buf_start,
        buf_end: line.buf_end,
        chars,
        checkbox: None,
    });
}

/// Paint a code block: measure the lines, draw the box, then paint the lines.
fn paint_code_block(
    ui: &mut Ui,
    buffer: &str,
    atoms: &[Atom],
    block: &BlockGroup,
    dark: bool,
    out: &mut Vec<LinePaint>,
) {
    let bg = if dark { Color32::from_rgb(38, 38, 44) } else { Color32::from_rgb(244, 244, 246) };
    let spacing = ui.spacing().item_spacing.y;
    let wrap = (ui.available_width() - 2.0 * CODE_INDENT).max(40.0);
    // Pass 1: lay out galleys to measure total height (as allocated below).
    let galleys: Vec<_> = block
        .lines
        .iter()
        .map(|line| {
            let (job, chars) = build_line_job(buffer, atoms, line, dark, true, wrap);
            (ui.fonts_mut(|f| f.layout_job(job)), chars, line)
        })
        .collect();
    let rows_h: f32 = galleys.iter().map(|(g, ..)| g.size().y.max(BASE - 2.0)).sum::<f32>()
        + spacing * galleys.len().saturating_sub(1) as f32;
    let origin = ui.cursor().min;
    let width = ui.available_width();
    ui.painter().rect_filled(Rect::from_min_size(origin, Vec2::new(width, rows_h + 2.0 * CODE_PAD)), 6.0, bg);
    // Pass 2: allocate + paint each line (indented inside the box). The
    // padding is allocated too, so the next block starts below the box.
    ui.add_space(CODE_PAD);
    for (g, chars, line) in galleys {
        let size = g.size();
        let (_, rect) = ui.allocate_space(Vec2::new(size.x.max(1.0), size.y.max(BASE - 2.0)));
        let line_origin = rect.min + Vec2::new(CODE_INDENT, 0.0);
        ui.painter().galley(line_origin, g.clone(), text_color(dark));
        out.push(LinePaint {
            galley: g,
            origin: line_origin,
            height: size.y.max(BASE - 2.0),
            buf_start: line.buf_start,
            buf_end: line.buf_end,
            chars,
            checkbox: None,
        });
    }
    ui.add_space(CODE_PAD + 2.0);
}

/// Paint the selection highlight, row by row (lines may wrap).
fn paint_selection(ui: &Ui, lines: &[LinePaint], selection: Option<(usize, usize)>, dark: bool) {
    let Some((s, e)) = selection else { return };
    let (lo, hi) = (s.min(e), s.max(e));
    let sel_color = if dark {
        Color32::from_rgba_unmultiplied(90, 110, 160, 120)
    } else {
        Color32::from_rgba_unmultiplied(120, 150, 220, 90)
    };
    let painter = ui.painter();
    for line in lines {
        if hi < line.buf_start || lo > line.buf_end {
            continue;
        }
        let g0 = line.galley_index(lo.max(line.buf_start));
        let g1 = line.galley_index(hi.min(line.buf_end));
        // The selection runs on past this line's newline: show a sliver for it.
        let past_end = hi > line.buf_end;
        let mut row_start = 0;
        for row in &line.galley.rows {
            let row_end = row_start + row.char_count_including_newline().0;
            let (a, b) = (g0.max(row_start), g1.min(row_end));
            let last_row = row_end >= line.galley.end().index.0;
            if a < b || (past_end && last_row && a <= b) {
                // x of the char boundary `i` within this row; a cursor at a
                // row boundary would report the previous row's end instead.
                let x_at = |i: usize| match row.glyphs.get(i - row_start) {
                    Some(g) => row.pos.x + g.pos.x,
                    None => row.rect().max.x,
                };
                let x0 = x_at(a);
                let mut x1 = x_at(b);
                if past_end && last_row {
                    x1 += 6.0;
                }
                let r = row.rect();
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, r.min.y), Pos2::new(x1, r.max.y)).translate(line.origin.to_vec2()),
                    0.0,
                    sel_color,
                );
            }
            row_start = row_end;
        }
    }
}

// --- line/block grouping ----------------------------------------------------

/// One buffer line's atoms. Fully hidden lines (fence markers, rules) are
/// folded into the following line so they don't paint as blank rows.
struct LineGroup {
    atoms: std::ops::Range<usize>,
    buf_start: usize,
    buf_end: usize,
}

/// A line separator is a hidden atom starting with '\n'. (Hidden inline
/// markers like '`' or '[' are also one byte long — they must not split.)
fn is_newline(buffer: &str, a: &Atom) -> bool {
    matches!(a, Atom::Hidden { range } if buffer.as_bytes().get(range.start) == Some(&b'\n'))
}

fn group_lines(buffer: &str, atoms: &[Atom]) -> Vec<LineGroup> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, a) in atoms.iter().enumerate() {
        if is_newline(buffer, a) || i + 1 == atoms.len() {
            let range = start..i + 1;
            let group = &atoms[range.clone()];
            let has_text = group.iter().any(|a| matches!(a, Atom::Text { .. }));
            // Skip hidden-only tails (a closing fence or rule at the doc end).
            if has_text || is_newline(buffer, a) {
                let buf_start = atom_range(&group[0]).start;
                let buf_end = if is_newline(buffer, a) { atom_range(a).start } else { atom_range(a).end };
                lines.push(LineGroup { atoms: range, buf_start, buf_end });
            }
            start = i + 1;
        }
    }
    lines
}

fn atom_range(a: &Atom) -> &std::ops::Range<usize> {
    match a {
        Atom::Hidden { range } | Atom::Text { range, .. } => range,
    }
}

fn group_blocks(lines: Vec<LineGroup>, atoms: &[Atom]) -> Vec<BlockGroup> {
    let mut blocks: Vec<BlockGroup> = Vec::new();
    for line in lines {
        let kind = line_kind(&line, atoms);
        match blocks.last_mut() {
            Some(b) if b.kind == kind && kind != BlockKind::Single => b.lines.push(line),
            _ => blocks.push(BlockGroup { lines: vec![line], kind }),
        }
    }
    blocks
}

fn line_kind(line: &LineGroup, atoms: &[Atom]) -> BlockKind {
    for a in &atoms[line.atoms.clone()] {
        if let Atom::Text { chrome, .. } = a {
            return match chrome {
                BlockChrome::Code => BlockKind::Code,
                BlockChrome::ListBullet { .. } => BlockKind::List,
                BlockChrome::Quote(_) => BlockKind::Quote,
                BlockChrome::Table { .. } => BlockKind::Table,
                _ => BlockKind::Single,
            };
        }
    }
    BlockKind::Single
}
