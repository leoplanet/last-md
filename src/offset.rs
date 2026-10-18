//! Stage 2: offset map (buffer byte <-> rendered index) and caret model.
//!
//! The WYSIWYG layout skips hidden markers, so the rendered text is shorter
//! than the buffer. `OffsetMap` maps between the two. A list bullet glyph is
//! inserted into the rendered stream (it has no buffer char) and maps back to
//! the item's first content byte.
//!
//! Pure logic, no GUI — unit-tested.

use crate::wysiwyg::{Atom, BlockChrome};

/// Bidirectional mapping between buffer byte offsets and rendered indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OffsetMap {
    /// buffer byte -> rendered index (hidden bytes map to the caret slot).
    pub buf_to_render: Vec<usize>,
    /// rendered index -> buffer byte (chrome glyphs map to the nearest byte).
    pub render_to_buf: Vec<usize>,
}

impl OffsetMap {
    pub fn from_atoms(buffer: &str, atoms: &[Atom]) -> Self {
        let n = buffer.len();
        let mut buf_to_render = vec![0usize; n];
        let mut render_to_buf: Vec<usize> = Vec::new();
        let mut rendered = 0usize;

        for atom in atoms {
            match atom {
                Atom::Hidden { range } => {
                    // Hidden bytes map to the current rendered slot (where the
                    // caret would sit). No rendered char is emitted.
                    for b in range.clone() {
                        if b < n {
                            buf_to_render[b] = rendered;
                        }
                    }
                }
                Atom::Text { range, chrome, .. } => {
                    // A list bullet is a rendered glyph with no buffer char;
                    // it maps back to the item's first content byte.
                    if matches!(chrome, BlockChrome::ListBullet { .. }) {
                        let first = range.start.min(n.saturating_sub(1));
                        render_to_buf.push(first);
                        rendered += 1;
                    }
                    let text = buffer.get(range.clone()).unwrap_or("");
                    for (rel, _ch) in text.char_indices() {
                        let byte = range.start + rel;
                        if byte < n {
                            buf_to_render[byte] = rendered;
                        }
                        render_to_buf.push(byte);
                        rendered += 1;
                    }
                }
            }
        }
        OffsetMap {
            buf_to_render,
            render_to_buf,
        }
    }

    /// Rendered index for a buffer byte (caret display position).
    pub fn render_of_buf(&self, b: usize) -> usize {
        *self.buf_to_render.get(b).unwrap_or(&0)
    }

    /// Buffer byte for a rendered index (mouse -> caret).
    pub fn buf_of_render(&self, r: usize) -> usize {
        *self.render_to_buf.get(r).unwrap_or(&0)
    }

    pub fn rendered_len(&self) -> usize {
        self.render_to_buf.len()
    }

    /// The buffer length this map was built from.
    pub fn buf_len(&self) -> usize {
        self.buf_to_render.len()
    }
}

/// If `caret` (a buffer byte) falls inside a hidden marker range, jump to the
/// range's END (forward). Otherwise return it unchanged.
pub fn skip_hidden_forward(caret: usize, atoms: &[Atom]) -> usize {
    for atom in atoms {
        if let Atom::Hidden { range } = atom {
            if caret >= range.start && caret < range.end {
                return range.end;
            }
        }
    }
    caret
}

/// If `caret` falls inside a hidden marker range, jump to the range's START
/// (backward). Otherwise return it unchanged.
pub fn skip_hidden_backward(caret: usize, atoms: &[Atom]) -> usize {
    for atom in atoms {
        if let Atom::Hidden { range } = atom {
            if caret > range.start && caret <= range.end {
                return range.start;
            }
        }
    }
    caret
}

/// True if buffer byte `b` is part of a visible (Text) atom.
fn is_visible(b: usize, atoms: &[Atom]) -> bool {
    atoms
        .iter()
        .any(|a| matches!(a, Atom::Text { range, .. } if range.start <= b && b < range.end))
}

/// The byte offset of the last visible character strictly before `pos`,
/// skipping any hidden marker bytes between it and `pos`. Used by backspace so
/// deleting a visible char also removes the hidden markers that follow it.
pub fn prev_visible_char_start(buffer: &str, atoms: &[Atom], pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut cur = buffer[..pos].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
    loop {
        if is_visible(cur, atoms) {
            return cur;
        }
        if cur == 0 {
            return 0;
        }
        cur = buffer[..cur].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
    }
}

/// Move the caret one grapheme in `dir` (+1 right, -1 left), then skip any
/// hidden marker it lands on.
pub fn caret_step(caret: usize, dir: i32, atoms: &[Atom], buffer: &str) -> usize {
    let n = buffer.len();
    let pos = caret.min(n);
    if dir > 0 {
        if pos >= n {
            return pos;
        }
        let next = pos + buffer[pos..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        skip_hidden_forward(next.min(n), atoms)
    } else {
        if pos == 0 {
            return 0;
        }
        // Start of the char ending at `pos`.
        let prev = buffer[..pos].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
        skip_hidden_backward(prev, atoms)
    }
}

/// True if buffer byte `b` is a place the caret may rest: before a visible
/// char, at a line end (the '\n' that ends a painted line), or at the end of
/// the buffer. Every on-screen caret position has exactly one such byte, so
/// arrow keys never "stall" on hidden markers and never skip a line end.
pub fn is_stop(buffer: &str, atoms: &[Atom], b: usize) -> bool {
    if b >= buffer.len() {
        return b == buffer.len();
    }
    if !buffer.is_char_boundary(b) {
        return false;
    }
    atoms.iter().any(|a| match a {
        // An empty table cell / list item is a stop at its zero-width atom.
        Atom::Text { range, .. } => (range.start <= b && b < range.end) || (range.is_empty() && range.start == b),
        Atom::Hidden { range } => range.start == b && buffer.as_bytes()[b] == b'\n',
    })
}

/// Snap `b` forward to the nearest caret stop (itself if it is one).
pub fn snap_forward(buffer: &str, atoms: &[Atom], b: usize) -> usize {
    let mut b = b.min(buffer.len());
    while !is_stop(buffer, atoms, b) {
        b += 1;
    }
    b
}

/// The next caret stop in `dir` (+1 right, -1 left), or `caret` if none.
pub fn next_stop(buffer: &str, atoms: &[Atom], caret: usize, dir: i32) -> usize {
    let caret = caret.min(buffer.len());
    if dir > 0 {
        if caret >= buffer.len() {
            return caret;
        }
        snap_forward(buffer, atoms, caret + 1)
    } else {
        let mut b = caret;
        while b > 0 {
            b -= 1;
            if is_stop(buffer, atoms, b) {
                return b;
            }
        }
        caret
    }
}
