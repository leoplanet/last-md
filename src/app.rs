//! Last-MD: a minimal WYSIWYG markdown editor (Typora-clone MVP).
//!
//! Single-buffer architecture: the document is one `String`; the WYSIWYG view
//! (`layout::paint`) renders it with markers hidden and block chrome drawn.
//! The caret and selection are buffer offsets, so cross-block and whole-doc
//! selection work naturally.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{
    Align2, Color32, Event, FontId, Vec2, FontData, FontDefinitions, FontFamily, Key, Pos2, PointerButton, Rect, RichText, Ui,
};
use crate::layout;
use crate::layout::{LinePaint, BOLD_FAMILY};
use crate::md;
use crate::offset::{next_stop, prev_visible_char_start, snap_forward};
use crate::wysiwyg::{code_blocks, fence_open, scan, scan_at, tables, Atom, CodeBlock, TableBlock};

pub fn setup_fonts(ctx: &eframe::egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data
        .insert("Inter".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/Inter-Regular.ttf"))));
    fonts.font_data
        .insert(BOLD_FAMILY.to_string(), Arc::new(FontData::from_static(include_bytes!("../fonts/Inter-Bold.ttf"))));
    fonts.font_data.insert(
        "JetBrainsMono".into(),
        Arc::new(FontData::from_static(include_bytes!("../fonts/JetBrainsMono-Regular.ttf"))),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "JetBrainsMono".into());
    // The layout asks for FontFamily::Name("InterBold"); without a family
    // binding egui falls back to nothing and bold text renders as boxes.
    fonts
        .families
        .insert(FontFamily::Name(BOLD_FAMILY.into()), vec![BOLD_FAMILY.to_string(), "Inter".into()]);
    ctx.set_fonts(fonts);
    for theme in [eframe::egui::Theme::Dark, eframe::egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.text_styles.insert(
                eframe::egui::TextStyle::Body,
                eframe::egui::FontId::new(18.0, FontFamily::Proportional),
            );
        });
    }
}

pub fn apply_visuals(ctx: &eframe::egui::Context, dark: bool) {
    let mut v = if dark {
        eframe::egui::Visuals::dark()
    } else {
        eframe::egui::Visuals::light()
    };
    if dark {
        v.override_text_color = Some(Color32::from_rgb(232, 232, 236));
        v.weak_text_color = Some(Color32::from_rgb(150, 150, 160));
        v.window_fill = Color32::from_rgb(26, 26, 30);
        v.panel_fill = Color32::from_rgb(26, 26, 30);
        v.extreme_bg_color = Color32::from_rgb(26, 26, 30);
        v.code_bg_color = Color32::from_rgb(38, 38, 44);
    } else {
        v.override_text_color = Some(Color32::from_rgb(28, 28, 32));
        v.weak_text_color = Some(Color32::from_rgb(120, 120, 130));
        v.window_fill = Color32::from_rgb(250, 250, 250);
        v.panel_fill = Color32::from_rgb(250, 250, 250);
        v.extreme_bg_color = Color32::from_rgb(250, 250, 250);
        v.code_bg_color = Color32::from_rgb(244, 244, 246);
    }
    ctx.set_visuals(v);
}

/// Caret blink half-period.
const BLINK: std::time::Duration = std::time::Duration::from_millis(530);

const AUTOSAVE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

pub struct AppModel {
    /// The single markdown buffer (source of truth).
    pub buffer: String,
    /// Caret as a buffer byte offset.
    pub caret: usize,
    /// Selection as a buffer byte range (None = no selection).
    pub selection: Option<(usize, usize)>,
    /// Anchor of an in-progress mouse drag (buffer byte).
    drag_anchor: Option<usize>,
    /// Screen rects of the last painted runs (for caret/selection tests).
    pub last_runs: Vec<LinePaint>,
    last_caret: Option<Rect>,
    /// Screen x the caret aims for across consecutive Up/Down presses.
    goal_x: Option<f32>,
    /// When the caret last moved (or the text changed): blinking restarts
    /// from "on" so the caret is always visible right after an action.
    caret_since: Instant,
    caret_seen: (usize, usize),
    /// Screen rect of the document viewport (inside the scroll area,
    /// excluding the scroll bar) as of the last frame.
    doc_area: Option<Rect>,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
    source_mode: bool,
    source_buf: String,
    path: Option<PathBuf>,
    dirty: bool,
    last_change: Instant,
    find: String,
    find_open: bool,
    /// Focus the find box on its next frame (Ctrl+F).
    find_focus: bool,
    focus_mode: bool,
    dark: bool,
}

impl AppModel {
    pub fn new() -> Self {
        AppModel {
            buffer: String::new(),
            caret: 0,
            selection: None,
            drag_anchor: None,
            last_runs: Vec::new(),
            last_caret: None,
            goal_x: None,
            caret_since: Instant::now(),
            caret_seen: (0, 0),
            doc_area: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            source_mode: false,
            source_buf: String::new(),
            path: None,
            dirty: false,
            last_change: Instant::now(),
            find: String::new(),
            find_open: false,
            find_focus: false,
            focus_mode: false,
            dark: true,
        }
    }

    /// Screen rect of the caret as last painted.
    pub fn caret_screen_rect(&self) -> Option<Rect> {
        self.last_caret
    }

    pub fn current_doc(&self) -> String {
        self.buffer.clone()
    }

    pub fn load_doc(&mut self, text: &str) {
        self.buffer = text.to_string();
        self.caret = 0;
        self.selection = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = false;
        self.last_change = Instant::now();
    }

    fn mark_changed(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
    }

    /// Snapshot the buffer before an edit (undo).
    fn commit(&mut self) {
        self.undo_stack.push(self.buffer.clone());
        self.undo_stack.truncate(200);
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.buffer.clone());
            self.buffer = prev;
            self.caret = self.caret.min(self.buffer.len());
            self.selection = None;
            self.mark_changed();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.buffer.clone());
            self.buffer = next;
            self.caret = self.caret.min(self.buffer.len());
            self.selection = None;
            self.mark_changed();
        }
    }

    fn open_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.open_path(path);
        }
    }

    /// Load a file (a missing one opens empty and is created on save).
    pub fn open_path(&mut self, path: PathBuf) {
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        self.load_doc(&src);
        self.source_buf = src;
        self.path = Some(path);
    }

    fn new_doc(&mut self) {
        self.load_doc("");
        self.source_buf.clear();
        self.path = None;
    }

    fn save(&mut self) {
        if self.path.is_none() {
            self.path = rfd::FileDialog::new().add_filter("Markdown", &["md"]).save_file();
        }
        if let Some(path) = &self.path {
            if std::fs::write(path, &self.buffer).is_ok() {
                self.dirty = false;
            }
        }
    }

    fn toggle_source(&mut self) {
        self.source_mode = !self.source_mode;
        if self.source_mode {
            self.source_buf = self.buffer.clone();
        } else {
            // render_source keeps `buffer` in sync; just keep the caret valid.
            self.caret = self.caret.min(self.buffer.len());
            self.selection = None;
        }
    }

    fn toggle_theme(&mut self, ctx: &eframe::egui::Context) {
        self.dark = !self.dark;
        apply_visuals(ctx, self.dark);
    }

    // --- editing ---------------------------------------------------------

    /// Insert `text`, replacing the selection if any. Returns true if changed.
    pub fn insert_text(&mut self, text: &str) -> bool {
        self.commit();
        let (lo, hi) = match self.selection {
            Some((a, b)) => (a.min(b), a.max(b)),
            None => (self.caret, self.caret),
        };
        self.buffer.replace_range(lo..hi, text);
        self.caret = lo + text.len();
        self.selection = None;
        self.mark_changed();
        true
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection {
            self.commit();
            let (lo, hi) = (a.min(b), a.max(b));
            self.buffer.replace_range(lo..hi, "");
            self.caret = lo;
            self.selection = None;
            self.mark_changed();
            true
        } else {
            false
        }
    }

    fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let atoms = scan_at(&self.buffer, self.caret);
        if self.caret == 0 {
            return;
        }
        if let Some(block) = self.code_block_at() {
            if self.caret == block.open.end {
                // Start of the first code line: never merge into the fence.
                // In an empty block, Backspace removes the block.
                if block.open.end + 1 == block.close.start {
                    self.commit();
                    self.buffer.replace_range(block.open.start..block.close.end, "");
                    self.caret = block.open.start;
                    self.mark_changed();
                }
                return;
            }
        }
        let (ls, _) = line_bounds(&self.buffer, self.caret);
        let from = if self.caret == ls {
            // Line start: join with the previous line (delete the '\n').
            ls - 1
        } else {
            // Only a hidden block marker before the caret on this line (e.g.
            // "# " or "- "): drop the marker, turning the block into a
            // paragraph, rather than reaching into the previous line.
            prev_visible_char_start(&self.buffer, &atoms, self.caret).max(ls)
        };
        self.commit();
        if from < self.caret {
            self.buffer.replace_range(from..self.caret, "");
            self.caret = from;
            self.mark_changed();
        }
    }

    /// Forward delete: remove the char at the caret (a '\n' joins lines).
    fn delete_forward(&mut self) {
        if self.delete_selection() || self.caret >= self.buffer.len() {
            return;
        }
        self.commit();
        let len = self.buffer[self.caret..].chars().next().map_or(1, |c| c.len_utf8());
        self.buffer.replace_range(self.caret..self.caret + len, "");
        self.mark_changed();
    }

    fn enter(&mut self) {
        self.delete_selection();
        // Find the current line's byte range.
        let (line_start, line_end) = line_bounds(&self.buffer, self.caret);
        let line = self.buffer[line_start..line_end].to_string();
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if let Some(block) = self.code_block_at() {
            // Enter on an empty last code line (after at least one other
            // line) drops that line and leaves the block, like an empty list
            // item. Otherwise it's a plain newline that keeps indentation.
            let empty_last = line.is_empty() && line_end + 1 == block.close.start && line_start > block.open.end;
            if empty_last {
                self.commit();
                self.buffer.replace_range(line_start..line_end + 1, "");
                self.mark_changed();
                self.caret = line_start;
                if let Some(block) = code_blocks(&self.buffer).into_iter().find(|b| b.open.start == block.open.start) {
                    self.exit_code_block(&block);
                }
            } else {
                self.insert_text(&format!("\n{indent}"));
            }
            return;
        }
        // In a table, Enter adds an empty row below and moves into it.
        if let Some(table) = self.table_at() {
            self.add_table_row(&table, line_end);
            return;
        }
        // Enter on a ``` line that isn't a block yet: open a closed block
        // with the caret on its (empty) first line.
        if let Some((fc, flen)) = fence_open(&line) {
            if self.caret == line_end && !code_blocks(&self.buffer).iter().any(|b| b.open.start == line_start) {
                self.commit();
                let close: String = std::iter::repeat_n(fc, flen).collect();
                self.buffer.insert_str(line_end, &format!("\n\n{indent}{close}"));
                self.caret = line_end + 1;
                self.selection = None;
                self.mark_changed();
                return;
            }
        }

        self.commit();
        let (new_line, mut template) = md::on_enter(&line, None);
        if !template.is_empty() {
            // Keep a nested list item at its depth.
            template.insert_str(0, &indent);
        }
        let insert_at = self.caret;
        // Split: keep text before caret, add newline, template, then rest of line.
        let rest_of_line = &line[insert_at - line_start..];
        let mut newbuf = String::new();
        newbuf.push_str(&self.buffer[..line_start]);
        newbuf.push_str(&new_line[..(insert_at - line_start).min(new_line.len())]);
        newbuf.push('\n');
        newbuf.push_str(&template);
        newbuf.push_str(rest_of_line);
        newbuf.push_str(&self.buffer[line_end..]);
        self.buffer = newbuf;
        self.caret = line_start + (insert_at - line_start).min(new_line.len()) + 1 + template.len();
        self.selection = None;
        self.mark_changed();
    }

    fn tab(&mut self, out: bool) {
        self.delete_selection();
        self.commit();
        let (line_start, line_end) = line_bounds(&self.buffer, self.caret);
        let line = self.buffer[line_start..line_end].to_string();
        let new_line = md::indent(&line, out);
        let delta = new_line.len() as i64 - line.len() as i64;
        self.buffer.replace_range(line_start..line_end, &new_line);
        self.caret = (self.caret as i64 + delta).max(line_start as i64) as usize;
        self.mark_changed();
    }

    /// The closed code block whose code lines hold the caret.
    fn code_block_at(&self) -> Option<CodeBlock> {
        code_blocks(&self.buffer).into_iter().find(|b| b.holds_caret(self.caret))
    }

    /// Select the next (or previous) case-insensitive match of the find
    /// text, starting at `from` and wrapping around the document.
    fn find_next(&mut self, from: usize, backward: bool) {
        if self.find.is_empty() {
            return;
        }
        // ASCII lowercasing keeps byte offsets identical to the buffer's.
        let hay = self.buffer.to_ascii_lowercase();
        let needle = self.find.to_ascii_lowercase();
        let from = from.min(hay.len());
        let hit = if backward {
            hay[..from].rfind(&needle).or_else(|| hay.rfind(&needle))
        } else {
            hay[from..].find(&needle).map(|i| from + i).or_else(|| hay.find(&needle))
        };
        if let Some(start) = hit {
            self.selection = Some((start, start + needle.len()));
            self.caret = start + needle.len();
        }
    }

    /// The table whose rows hold the caret.
    fn table_at(&self) -> Option<TableBlock> {
        tables(&self.buffer).into_iter().find(|t| t.range.start <= self.caret && self.caret < t.range.end.max(t.range.start + 1)
            || (t.range.end == self.buffer.len() && self.caret == t.range.end))
    }

    /// Insert an empty row after the row ending at `line_end`; caret into
    /// its first cell.
    fn add_table_row(&mut self, table: &TableBlock, line_end: usize) {
        let row = format!("\n|{}", "  |".repeat(table.aligns.len()));
        self.commit();
        self.buffer.insert_str(line_end, &row);
        self.caret = line_end + 3;
        self.selection = None;
        self.mark_changed();
    }

    /// Tab / Shift+Tab: caret to the next / previous cell (Tab past the last
    /// cell adds a row).
    fn table_cell_step(&mut self, forward: bool) {
        let Some(table) = self.table_at() else { return };
        let atoms = scan_at(&self.buffer, self.caret);
        // Cell starts: the first caret stop after each cell-opening pipe.
        let mut starts = Vec::new();
        let mut ls = table.range.start;
        while ls < table.range.end {
            let le = self.buffer[ls..].find('\n').map_or(self.buffer.len(), |i| ls + i);
            if ls != table.delim.start {
                let line = &self.buffer[ls..le];
                let pipes: Vec<usize> = line.match_indices('|').map(|(i, _)| ls + i).collect();
                for w in pipes.windows(2) {
                    starts.push(snap_forward(&self.buffer, &atoms, w[0] + 1).min(w[1]));
                }
            }
            ls = le + 1;
        }
        let cur = starts.iter().rposition(|&s| s <= self.caret).unwrap_or(0);
        if forward {
            match starts.get(cur + 1) {
                Some(&s) => self.caret = s,
                None => {
                    let end = table.range.end - usize::from(self.buffer[..table.range.end].ends_with('\n'));
                    self.add_table_row(&table, end);
                }
            }
        } else if cur > 0 {
            self.caret = starts[cur - 1];
        }
        self.selection = None;
    }

    /// Leave a code block: caret to the start of the line after its closing
    /// fence, adding that line if the block ends the document.
    fn exit_code_block(&mut self, block: &CodeBlock) {
        if !self.buffer[block.close.clone()].ends_with('\n') {
            self.commit();
            self.buffer.push('\n');
            self.mark_changed();
        }
        self.caret = block.close.start + self.buffer[block.close.start..].find('\n').map_or(0, |i| i + 1);
        self.selection = None;
        self.goal_x = None;
    }

    /// Move the caret to `to`; with `extend`, grow the selection from its
    /// anchor (or from the caret if there is none).
    fn move_caret(&mut self, to: usize, extend: bool) {
        if extend {
            let anchor = self.selection.map_or(self.caret, |(a, _)| a);
            self.selection = (anchor != to).then_some((anchor, to));
        } else {
            self.selection = None;
        }
        self.caret = to;
    }

    fn select_all(&mut self) {
        self.selection = Some((0, self.buffer.len()));
    }

    // --- update ----------------------------------------------------------

    pub fn update(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        apply_visuals(ctx, self.dark);

        // Global shortcuts.
        let input = ctx.input(|i| {
            (
                i.key_pressed(Key::S) && i.modifiers.ctrl,
                i.key_pressed(Key::O) && i.modifiers.ctrl,
                i.key_pressed(Key::N) && i.modifiers.ctrl,
                i.key_pressed(Key::Slash) && i.modifiers.ctrl,
                i.key_pressed(Key::F) && i.modifiers.ctrl,
                i.key_pressed(Key::D) && i.modifiers.ctrl,
                i.key_pressed(Key::F8),
                i.key_pressed(Key::A) && i.modifiers.ctrl,
                i.key_pressed(Key::Z) && i.modifiers.ctrl && !i.modifiers.shift,
                (i.key_pressed(Key::Y) && i.modifiers.ctrl) || (i.key_pressed(Key::Z) && i.modifiers.ctrl && i.modifiers.shift),
            )
        });
        if input.0 {
            self.save();
        } else if input.1 {
            self.open_file();
        } else if input.2 {
            self.new_doc();
        } else if input.3 {
            self.toggle_source();
        } else if input.4 {
            self.find_open = true;
            self.find_focus = true;
        } else if input.5 {
            self.toggle_theme(&ctx);
        } else if input.6 {
            self.focus_mode = !self.focus_mode;
        } else if input.7 {
            self.select_all();
        } else if input.8 && !self.source_mode {
            // (Source mode's TextEdit has its own undo.)
            self.undo();
        } else if input.9 && !self.source_mode {
            self.redo();
        }

        // Autosave.
        if self.dirty && self.path.is_some() && self.last_change.elapsed() > AUTOSAVE_AFTER {
            self.save();
        }

        // Find panel.
        if self.find_open {
            eframe::egui::Panel::bottom("find").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("find").weak());
                    let resp = ui.add(eframe::egui::TextEdit::singleline(&mut self.find).desired_width(200.0));
                    if std::mem::take(&mut self.find_focus) {
                        resp.request_focus();
                    }
                    let (enter, shift, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift, i.key_pressed(Key::Escape)));
                    if resp.changed() {
                        // Search as you type, from the current match's start.
                        let from = self.selection.map_or(self.caret, |(a, b)| a.min(b));
                        self.find_next(from, false);
                    }
                    if resp.lost_focus() && enter {
                        // Enter / Shift+Enter: next / previous match; keep typing.
                        let from = match self.selection {
                            Some((a, b)) if !shift => a.max(b),
                            Some((a, b)) => a.min(b),
                            None => self.caret,
                        };
                        self.find_next(from, shift);
                        resp.request_focus();
                    }
                    if ui.button("close").clicked() || (escape && resp.has_focus()) {
                        self.find_open = false;
                    }
                });
            });
        }

        // Status bar (panels go before the central panel, or they overlap it).
        eframe::egui::Panel::bottom("status").show(ui, |ui| {
            let (words, chars, _lines) = md::word_count(&self.buffer);
            ui.horizontal(|ui| {
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    if self.source_mode {
                        ui.label(RichText::new("source").size(11.0).weak());
                    }
                    if self.focus_mode {
                        ui.label(RichText::new("focus").size(11.0).weak());
                    }
                    if self.dirty {
                        ui.label(RichText::new("●").size(11.0).weak());
                    }
                    let name = match &self.path {
                        Some(p) => p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                        None => "untitled".to_string(),
                    };
                    ui.label(RichText::new(name).size(11.0).weak())
                        .on_hover_text(format!("{words} words · {chars} chars"));
                });
            });
        });

        // Document.
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            if self.source_mode {
                eframe::egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| self.render_source(ui));
                return;
            }
            let pad = 16.0;
            // Input first, against last frame's layout (positions don't move
            // between frames), so this frame paints the caret where the input
            // put it. Painting first left the caret one action behind until
            // something else triggered a repaint.
            let area = self.doc_area.unwrap_or(ui.max_rect());
            let lines = std::mem::take(&mut self.last_runs);
            self.handle_mouse(ui, area, &lines);
            self.handle_keyboard(ui, &lines);
            let mut atoms = scan_at(&self.buffer, self.caret);
            let snapped = snap_forward(&self.buffer, &atoms, self.caret);
            if snapped != self.caret {
                // Revealed markers depend on the caret: rescan where it landed.
                self.caret = snapped;
                atoms = scan_at(&self.buffer, self.caret);
            }
            let seen = (self.caret, self.buffer.len());
            let caret_moved = seen != self.caret_seen;
            if caret_moved {
                self.caret_seen = seen;
                self.caret_since = Instant::now();
            }
            let phase = self.caret_since.elapsed().as_millis() / BLINK.as_millis();
            let caret_on = phase % 2 == 0;
            let to_flip = BLINK.as_millis() - self.caret_since.elapsed().as_millis() % BLINK.as_millis();
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(to_flip as u64));
            let scroll = eframe::egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width() - 2.0 * pad);
                        ui.add_space(24.0);
                        let (runs, caret_rect) = layout::paint(ui, &self.buffer, &atoms, self.caret, caret_on, self.selection);
                        // Keep the caret in view when it moves (typing, keys).
                        if let (true, Some(r)) = (caret_moved, caret_rect) {
                            ui.scroll_to_rect(r.expand2(Vec2::new(0.0, 12.0)), None);
                        }
                        ui.add_space(48.0);
                        self.last_runs = runs;
                        self.last_caret = caret_rect;
                    });
                });
            });
            self.doc_area = Some(scroll.inner_rect);
            // Byline: top right, fixed, barely above the background.
            let dark = ui.visuals().dark_mode;
            let color = if dark { Color32::from_rgb(52, 52, 56) } else { Color32::from_rgb(222, 222, 226) };
            let corner = ui.max_rect().right_top() + Vec2::new(-14.0, 8.0);
            ui.painter().text(corner, Align2::RIGHT_TOP, "by Leo Choudhary", FontId::proportional(11.0), color);
        });
    }

    fn render_source(&mut self, ui: &mut Ui) {
        ui.add_space(16.0);
        let resp = ui.add(
            eframe::egui::TextEdit::multiline(&mut self.source_buf)
                .code_editor()
                .desired_width(f32::INFINITY),
        );
        if resp.changed() {
            self.commit();
            self.buffer = self.source_buf.clone();
            self.mark_changed();
        }
    }

    fn handle_mouse(&mut self, ui: &Ui, area: Rect, lines: &[LinePaint]) {
        let (pressed, down, released, pos, shift, double, triple) = ui.ctx().input(|i| {
            (
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.button_down(PointerButton::Primary),
                i.pointer.button_released(PointerButton::Primary),
                i.pointer.interact_pos(),
                i.modifiers.shift,
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_triple_clicked(PointerButton::Primary),
            )
        });
        if let Some(pos) = pos {
            if pressed && area.contains(pos) {
                // Task checkbox: toggle it, don't move the caret.
                if let Some(line) = lines.iter().find(|l| l.checkbox.is_some_and(|r| r.contains(pos))) {
                    self.toggle_task_at(line.buf_start);
                    return;
                }
            }
            if (double || triple) && area.contains(pos) {
                if let Some(b) = layout::byte_at_pos(lines, pos) {
                    let atoms = scan(&self.buffer);
                    let (lo, hi) = if triple {
                        let (ls, le) = line_bounds(&self.buffer, b);
                        (snap_forward(&self.buffer, &atoms, ls), le)
                    } else {
                        word_bounds(&self.buffer, &atoms, b)
                    };
                    self.selection = (lo != hi).then_some((lo, hi));
                    self.caret = hi;
                    self.drag_anchor = None;
                }
                return;
            }
            if pressed && area.contains(pos) {
                if let Some(b) = layout::byte_at_pos(lines, pos) {
                    self.move_caret(b, shift);
                    self.drag_anchor = Some(self.selection.map_or(b, |(a, _)| a));
                    self.goal_x = None;
                }
            } else if down {
                if let (Some(anchor), Some(b)) = (self.drag_anchor, layout::byte_at_pos(lines, pos)) {
                    self.caret = b;
                    self.selection = (b != anchor).then_some((anchor, b));
                }
            }
        }
        if released {
            self.drag_anchor = None;
        }
    }

    fn handle_keyboard(&mut self, ui: &Ui, lines: &[LinePaint]) {
        // Another widget (the find box) has keyboard focus: not ours.
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        // Events in order, so fast typing + arrows in one frame stay in order.
        let events = ui.ctx().input(|i| i.events.clone());
        if std::env::var_os("LAST_MD_DEBUG_EVENTS").is_some() {
            for ev in &events {
                eprintln!("event: {ev:?}");
            }
        }
        for ev in events {
            match ev {
                Event::Text(t) if !t.is_empty() => {
                    self.insert_text(&t);
                    self.goal_x = None;
                }
                Event::Copy => self.copy(ui.ctx()),
                Event::Cut => {
                    self.copy(ui.ctx());
                    self.delete_selection();
                }
                Event::Paste(text) => self.paste(&text),
                // egui only maps these on Windows; Linux apps and terminals
                // (and Omarchy's Super+V in terminals) use them too.
                Event::Key { key: Key::Insert, pressed: true, modifiers, .. } if modifiers.shift && !modifiers.ctrl => {
                    self.paste_system();
                }
                Event::Key { key: Key::V, pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.shift => {
                    self.paste_system();
                }
                Event::Key { key: Key::Insert, pressed: true, modifiers, .. } if modifiers.ctrl && !modifiers.shift => {
                    self.copy(ui.ctx());
                }
                Event::Key { key: Key::Delete, pressed: true, modifiers, .. } if modifiers.shift && !modifiers.ctrl => {
                    self.copy(ui.ctx());
                    self.delete_selection();
                }
                Event::Key { key, pressed: true, modifiers, .. } if modifiers.command || modifiers.ctrl => {
                    self.on_ctrl_key(key, modifiers.shift, lines);
                }
                // Alt+Shift+5: strikethrough (Typora).
                Event::Key { key: Key::Num5, pressed: true, modifiers, .. } if modifiers.alt && modifiers.shift => {
                    self.toggle_inline("~~", "~~");
                }
                Event::Key { key, pressed: true, modifiers, .. } if !modifiers.alt => {
                    self.on_key(key, modifiers.shift, lines);
                }
                _ => {}
            }
        }
    }

    fn on_key(&mut self, key: Key, shift: bool, lines: &[LinePaint]) {
        let atoms = scan_at(&self.buffer, self.caret);
        if !matches!(key, Key::ArrowUp | Key::ArrowDown) {
            self.goal_x = None;
        }
        let sel = self.selection.map(|(a, b)| (a.min(b), a.max(b)));
        // Right at the end of a code block's last line, or Down from its last
        // row, leaves the block (creating a line after it at the doc end).
        if !shift && sel.is_none() {
            if let Some(block) = self.code_block_at() {
                let leaving = match key {
                    Key::ArrowRight => self.caret + 1 == block.close.start,
                    Key::ArrowDown => !self.vertical_target(lines, 1).is_some_and(|t| block.holds_caret(t)),
                    _ => false,
                };
                if leaving {
                    self.exit_code_block(&block);
                    return;
                }
            }
        }
        let target = match key {
            Key::ArrowLeft => Some(match sel {
                Some((lo, _)) if !shift => lo,
                _ => next_stop(&self.buffer, &atoms, self.caret, -1),
            }),
            Key::ArrowRight => Some(match sel {
                Some((_, hi)) if !shift => hi,
                _ => next_stop(&self.buffer, &atoms, self.caret, 1),
            }),
            Key::ArrowUp => self.vertical_target(lines, -1),
            Key::ArrowDown => self.vertical_target(lines, 1),
            Key::PageUp | Key::PageDown => {
                let page = self.doc_area.map_or(400.0, |r| r.height() - 40.0);
                let dir = if key == Key::PageUp { -1.0 } else { 1.0 };
                layout::caret_rect(lines, self.caret)
                    .and_then(|r| layout::byte_at_pos(lines, Pos2::new(r.min.x, r.center().y + dir * page)))
            }
            Key::Home => Some(snap_forward(&self.buffer, &atoms, line_bounds(&self.buffer, self.caret).0)),
            Key::End => Some(snap_forward(&self.buffer, &atoms, line_bounds(&self.buffer, self.caret).1)),
            Key::Backspace => {
                self.backspace();
                None
            }
            Key::Delete => {
                self.delete_forward();
                None
            }
            Key::Enter if !shift => {
                self.enter();
                None
            }
            Key::Tab if self.table_at().is_some() => {
                self.table_cell_step(!shift);
                None
            }
            Key::Tab => {
                self.tab(shift);
                None
            }
            Key::Escape => {
                self.selection = None;
                None
            }
            _ => None,
        };
        if let Some(t) = target {
            self.move_caret(t, shift);
        }
    }

    /// Ctrl/Cmd shortcuts handled inside the document.
    fn on_ctrl_key(&mut self, key: Key, shift: bool, lines: &[LinePaint]) {
        // Word motion looks past markers, so use the unrevealed scan (its
        // stops are stops in the revealed one too).
        let atoms = scan(&self.buffer);
        self.goal_x = None;
        match key {
            // Ctrl+Enter: jump out of a code block (Typora).
            Key::Enter => {
                if let Some(block) = self.code_block_at() {
                    self.exit_code_block(&block);
                }
            }
            Key::B => self.toggle_inline("**", "**"),
            Key::I => self.toggle_inline("*", "*"),
            Key::U => self.toggle_inline("<u>", "</u>"),
            Key::K if shift => {
                self.insert_text("```\n");
            }
            Key::K => self.make_link(),
            Key::Backtick if shift => self.toggle_inline("`", "`"),
            Key::Num0 => self.set_heading(0),
            Key::Num1 => self.set_heading(1),
            Key::Num2 => self.set_heading(2),
            Key::Num3 => self.set_heading(3),
            Key::Num4 => self.set_heading(4),
            Key::Num5 => self.set_heading(5),
            Key::Num6 => self.set_heading(6),
            Key::Q if shift => self.toggle_line_prefix("> "),
            Key::CloseBracket if shift => self.toggle_line_prefix("- "),
            Key::OpenBracket if shift => self.toggle_line_prefix("1. "),
            Key::ArrowLeft => {
                let t = word_left(&self.buffer, &atoms, self.caret);
                self.move_caret(t, shift);
            }
            Key::ArrowRight => {
                let t = word_right(&self.buffer, &atoms, self.caret);
                self.move_caret(t, shift);
            }
            Key::Home => self.move_caret(snap_forward(&self.buffer, &atoms, 0), shift),
            Key::End => self.move_caret(self.buffer.len(), shift),
            Key::Backspace => {
                if self.selection.is_none() {
                    let from = word_left(&self.buffer, &atoms, self.caret);
                    self.selection = (from < self.caret).then_some((from, self.caret));
                }
                self.delete_selection();
            }
            Key::Delete => {
                if self.selection.is_none() {
                    let to = word_right(&self.buffer, &atoms, self.caret);
                    self.selection = (to > self.caret).then_some((self.caret, to));
                }
                self.delete_selection();
            }
            _ => {}
        }
        let _ = lines;
    }

    fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        if !text.is_empty() {
            self.insert_text(&text);
        }
        self.goal_x = None;
    }

    /// Paste for shortcuts egui doesn't turn into `Event::Paste` itself.
    fn paste_system(&mut self) {
        if let Some(text) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
            self.paste(&text);
        }
    }

    /// Copy the selection's markdown source to the clipboard.
    fn copy(&self, ctx: &eframe::egui::Context) {
        if let Some((a, b)) = self.selection {
            ctx.copy_text(self.buffer[a.min(b)..a.max(b)].to_string());
        }
    }

    /// Wrap the selection (or the word at the caret) in `open`/`close`, or
    /// unwrap it if it is already wrapped. With nothing to wrap, insert the
    /// pair with the caret between.
    fn toggle_inline(&mut self, open: &str, close: &str) {
        let atoms = scan(&self.buffer);
        let (lo, hi) = match self.selection {
            Some((a, b)) => (a.min(b), a.max(b)),
            None => word_bounds(&self.buffer, &atoms, self.caret),
        };
        self.commit();
        let wrapped = lo >= open.len()
            && self.buffer[..lo].ends_with(open)
            && self.buffer[hi..].starts_with(close)
            // `*` must not match half of a `**`.
            && !(open == "*" && self.buffer[..lo - 1].ends_with('*') && !self.buffer[..lo - 2].ends_with('*'));
        if wrapped {
            self.buffer.replace_range(hi..hi + close.len(), "");
            self.buffer.replace_range(lo - open.len()..lo, "");
            self.selection = (lo < hi).then_some((lo - open.len(), hi - open.len()));
            self.caret = hi - open.len();
        } else {
            self.buffer.insert_str(hi, close);
            self.buffer.insert_str(lo, open);
            self.selection = (lo < hi).then_some((lo + open.len(), hi + open.len()));
            self.caret = hi + open.len();
        }
        self.mark_changed();
    }

    /// Ctrl+K: turn the selection (or word) into `[text](url)` with the caret
    /// in the url.
    fn make_link(&mut self) {
        let atoms = scan(&self.buffer);
        let (lo, hi) = match self.selection {
            Some((a, b)) => (a.min(b), a.max(b)),
            None => word_bounds(&self.buffer, &atoms, self.caret),
        };
        self.commit();
        self.buffer.insert_str(hi, "]()");
        self.buffer.insert_str(lo, "[");
        self.caret = hi + 3;
        self.selection = None;
        self.mark_changed();
    }

    /// Ctrl+0..6: make the caret's line a heading of `level` (0 = paragraph);
    /// the same level again turns it back into a paragraph.
    fn set_heading(&mut self, level: usize) {
        let (ls, le) = line_bounds(&self.buffer, self.caret);
        let line = &self.buffer[ls..le];
        let hashes = line.chars().take_while(|&c| c == '#').count();
        let old = if hashes > 0 && line[hashes..].starts_with(' ') { hashes + 1 } else if hashes == line.len() { hashes } else { 0 };
        let level = if hashes == level && old > 0 { 0 } else { level };
        let new = if level == 0 { String::new() } else { format!("{} ", "#".repeat(level)) };
        self.commit();
        self.buffer.replace_range(ls..ls + old, &new);
        self.caret = (self.caret + new.len()).saturating_sub(old).max(ls + new.len());
        self.selection = None;
        self.mark_changed();
    }

    /// Toggle a block prefix (`> `, `- `, `1. `) on the caret's line.
    fn toggle_line_prefix(&mut self, prefix: &str) {
        let (ls, _) = line_bounds(&self.buffer, self.caret);
        self.commit();
        if self.buffer[ls..].starts_with(prefix) {
            self.buffer.replace_range(ls..ls + prefix.len(), "");
            self.caret = self.caret.saturating_sub(prefix.len()).max(ls);
        } else {
            self.buffer.insert_str(ls, prefix);
            self.caret += prefix.len();
        }
        self.selection = None;
        self.mark_changed();
    }

    /// Flip the `[ ]`/`[x]` box of the task item on the line at `ls`.
    fn toggle_task_at(&mut self, ls: usize) {
        let (ls, le) = line_bounds(&self.buffer, ls);
        let line = &self.buffer[ls..le];
        let Some(i) = line.find("[ ]").or_else(|| line.find("[x]")).or_else(|| line.find("[X]")) else { return };
        let mark = if &line[i + 1..i + 2] == " " { "x" } else { " " };
        self.commit();
        self.buffer.replace_range(ls + i + 1..ls + i + 2, mark);
        self.mark_changed();
    }

    /// Caret target one visual row up (`dir` -1) or down (+1), keeping the
    /// goal x across presses. Past the first/last row: doc start/end.
    fn vertical_target(&mut self, lines: &[LinePaint], dir: i32) -> Option<usize> {
        let cur = layout::caret_rect(lines, self.caret)?;
        let x = *self.goal_x.get_or_insert(cur.min.x);
        // Every visual row on screen, as (line, row top, row bottom).
        let rows = lines.iter().flat_map(|l| {
            l.galley.rows.iter().map(move |r| (l, l.origin.y + r.min_y(), l.origin.y + r.max_y()))
        });
        let next = if dir > 0 {
            rows.filter(|(_, top, _)| *top >= cur.max.y - 0.5).min_by(|a, b| a.1.total_cmp(&b.1))
        } else {
            rows.filter(|(_, _, bottom)| *bottom <= cur.min.y + 0.5).max_by(|a, b| a.2.total_cmp(&b.2))
        };
        Some(match next {
            Some((line, top, bottom)) => line.byte_at_pos(Pos2::new(x, (top + bottom) / 2.0)),
            None if dir > 0 => self.buffer.len(),
            None => 0,
        })
    }
}

/// The byte range (start, end) of the line containing `pos`.
fn line_bounds(buffer: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(buffer.len());
    let start = buffer[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = buffer[pos..].find('\n').map(|i| pos + i).unwrap_or(buffer.len());
    (start, end)
}

#[derive(PartialEq, Clone, Copy)]
enum CharClass {
    Space,
    Word,
    Punct,
}

/// Class of the visible char starting at caret stop `b` (line ends and the
/// doc end count as space).
fn class_at(buffer: &str, b: usize) -> CharClass {
    match buffer[b.min(buffer.len())..].chars().next() {
        None => CharClass::Space,
        Some(c) if c.is_whitespace() => CharClass::Space,
        Some(c) if c.is_alphanumeric() || c == '_' => CharClass::Word,
        Some(_) => CharClass::Punct,
    }
}

/// Ctrl+Left: start of the previous word, stepping over visible chars only
/// (hidden markers like `**` don't count as words).
fn word_left(buffer: &str, atoms: &[Atom], caret: usize) -> usize {
    let mut b = caret;
    let prev = |b: usize| next_stop(buffer, atoms, b, -1);
    while prev(b) < b && class_at(buffer, prev(b)) == CharClass::Space {
        b = prev(b);
    }
    if prev(b) == b {
        return b;
    }
    let cls = class_at(buffer, prev(b));
    while prev(b) < b && class_at(buffer, prev(b)) == cls {
        b = prev(b);
    }
    b
}

/// Ctrl+Right: past the current word, then past the spaces after it.
fn word_right(buffer: &str, atoms: &[Atom], caret: usize) -> usize {
    let mut b = caret;
    let next = |b: usize| next_stop(buffer, atoms, b, 1);
    let cls = class_at(buffer, b);
    if cls != CharClass::Space {
        while next(b) > b && class_at(buffer, b) == cls {
            b = next(b);
        }
    }
    while next(b) > b && class_at(buffer, b) == CharClass::Space && !buffer[b..].starts_with('\n') {
        b = next(b);
    }
    b
}

/// The word around buffer byte `b` (for double-click and Ctrl+B with no
/// selection); empty at a space.
fn word_bounds(buffer: &str, atoms: &[Atom], b: usize) -> (usize, usize) {
    let b = snap_forward(buffer, atoms, b);
    let at = class_at(buffer, b);
    let before = next_stop(buffer, atoms, b, -1);
    // At a word's end (e.g. caret after "word"), use the word before.
    let cls = if at == CharClass::Space && before < b { class_at(buffer, before) } else { at };
    if cls == CharClass::Space {
        return (b, b);
    }
    let (mut lo, mut hi) = (b, b);
    while next_stop(buffer, atoms, lo, -1) < lo && class_at(buffer, next_stop(buffer, atoms, lo, -1)) == cls {
        lo = next_stop(buffer, atoms, lo, -1);
    }
    while next_stop(buffer, atoms, hi, 1) > hi && class_at(buffer, hi) == cls {
        hi = next_stop(buffer, atoms, hi, 1);
    }
    // `hi` sits on the next stop; pull it back over hidden closing markers
    // to just after the last char of the word.
    let last = next_stop(buffer, atoms, hi, -1);
    if last >= lo && hi > lo {
        hi = last + buffer[last..].chars().next().map_or(0, |c| c.len_utf8());
    }
    (lo, hi)
}

impl eframe::App for AppModel {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.update(ui);
    }
}

/// Convenience for tests: build an AppModel from a buffer.
impl AppModel {
    pub fn from_buffer(text: &str) -> Self {
        let mut m = AppModel::new();
        m.buffer = text.to_string();
        m.caret = text.len();
        m
    }
}
