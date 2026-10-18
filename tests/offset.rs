//! Stage 2: offset map + caret model tests.
//!
//! These build on Stage 1's `scan` (atoms) and test the buffer<->rendered
//! mapping and caret-skipping over hidden markers.

use last_md::offset::{caret_step, skip_hidden_backward, skip_hidden_forward, OffsetMap};
use last_md::wysiwyg::scan;

fn map(buf: &str) -> OffsetMap {
    OffsetMap::from_atoms(buf, &scan(buf))
}

#[test]
fn offset_roundtrip_visible() {
    let buf = "a **b** c";
    let atoms = scan(buf);
    let m = map(buf);
    // Every visible byte round-trips.
    for atom in &atoms {
        if let last_md::wysiwyg::Atom::Text { range, .. } = atom {
            for b in range.clone() {
                assert_eq!(
                    m.render_to_buf[m.buf_to_render[b]],
                    b,
                    "visible byte {b} should round-trip"
                );
            }
        }
    }
}

#[test]
fn offset_hidden_maps_to_slot() {
    let buf = "a **b** c";
    let atoms = scan(buf);
    let m = map(buf);
    // The `**` markers are hidden; they map to a rendered index between the
    // visible 'a ' and 'b'.
    let hidden: Vec<usize> = atoms
        .iter()
        .filter_map(|a| if let last_md::wysiwyg::Atom::Hidden { range } = a { Some(range.clone()) } else { None })
        .flatten()
        .collect();
    assert!(!hidden.is_empty(), "expected hidden marker bytes");
    for b in hidden {
        let r = m.buf_to_render[b];
        // Hidden bytes map to a slot that is a valid rendered index.
        assert!(r < m.rendered_len() + 1, "hidden byte {b} maps to {r}");
    }
}

#[test]
fn offset_bullet_glyph() {
    let buf = "- item";
    let atoms = scan(buf);
    let m = map(buf);
    // The first rendered index should be the bullet, mapping to the item's
    // first content byte ('i' at byte 2).
    assert_eq!(m.render_to_buf[0], 2, "bullet should map to first content byte");
}

#[test]
fn caret_skip_forward() {
    let buf = "a **b** c";
    let atoms = scan(buf);
    // Caret on the first `*` (byte 2) jumps to after `**` (byte 4).
    assert_eq!(skip_hidden_forward(2, &atoms), 4);
    // Caret on visible text is unchanged.
    assert_eq!(skip_hidden_forward(0, &atoms), 0);
}

#[test]
fn caret_skip_backward() {
    let buf = "a **b** c";
    let atoms = scan(buf);
    // Caret just after `**` (byte 4) jumps back to before it (byte 2).
    assert_eq!(skip_hidden_backward(4, &atoms), 2);
}

#[test]
fn caret_step_across_marker() {
    let buf = "**b**";
    let atoms = scan(buf);
    // Caret at 0, step right: skips the `**` and lands on 'b' (byte 2).
    assert_eq!(caret_step(0, 1, &atoms, buf), 2);
}

#[test]
fn caret_at_doc_edges() {
    let buf = "abc";
    let atoms = scan(buf);
    assert_eq!(caret_step(0, -1, &atoms, buf), 0);
    assert_eq!(caret_step(3, 1, &atoms, buf), 3);
}
