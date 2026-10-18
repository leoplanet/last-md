//! Stage 1: source-aware scanner tests.

use last_md::md::Style;
use last_md::wysiwyg::{scan, Atom, BlockChrome};

fn visible(buffer: &str, atoms: &[Atom]) -> String {
    let mut s = String::new();
    for a in atoms {
        if let Atom::Text { range, .. } = a {
            s.push_str(&buffer[range.clone()]);
        }
    }
    s
}

/// Every byte covered exactly once, in order.
fn covers(buffer: &str, atoms: &[Atom]) -> bool {
    let mut cursor = 0usize;
    for a in atoms {
        let (s, e) = match a {
            Atom::Hidden { range } | Atom::Text { range, .. } => (range.start, range.end),
        };
        assert_eq!(s, cursor, "gap/overlap at {cursor}");
        cursor = e;
    }
    cursor == buffer.len()
}

fn hidden_spans(buffer: &str, atoms: &[Atom]) -> Vec<String> {
    atoms
        .iter()
        .filter_map(|a| if let Atom::Hidden { range } = a { Some(buffer[range.clone()].to_string()) } else { None })
        .collect()
}

#[test]
fn coverage_no_gaps_no_overlaps() {
    let b = "# Hello **world**\n- item one\n> quote\n```\ncode\n```\nplain text";
    let a = scan(b);
    assert!(covers(b, &a));
    // Code fence content IS visible (Code chrome); only markers are hidden.
    assert_eq!(visible(b, &a), "Hello worlditem onequotecodeplain text");
}

#[test]
fn scan_heading() {
    let b = "# Hi";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(hidden_spans(b, &a), vec!["# ".to_string()]);
    let text: Vec<&Atom> = a.iter().filter(|a| matches!(a, Atom::Text { .. })).collect();
    assert_eq!(text.len(), 1);
    if let Atom::Text { range, style, chrome } = &text[0] {
        assert_eq!(&b[range.clone()], "Hi");
        assert!(style.bold);
        assert_eq!(*chrome, BlockChrome::Heading(1));
    }
}

#[test]
fn scan_bold() {
    let b = "a **b** c";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "a b c");
    let bold = a.iter().find_map(|a| if let Atom::Text { range, style, .. } = a { if style.bold { Some(&b[range.clone()]) } else { None } } else { None });
    assert_eq!(bold, Some("b"));
}

#[test]
fn scan_italic_and_code_and_strike() {
    let b = "*it* `co` ~~st~~";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "it co st");
    assert!(a.iter().any(|a| matches!(a, Atom::Text { style, .. } if style.italic)));
    assert!(a.iter().any(|a| matches!(a, Atom::Text { style, .. } if style.code)));
    assert!(a.iter().any(|a| matches!(a, Atom::Text { style, .. } if style.strike)));
}

#[test]
fn scan_link() {
    let b = "go [x](u) now";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "go x now");
    let hidden = hidden_spans(b, &a);
    assert!(hidden.contains(&"[".to_string()));
    assert!(hidden.contains(&"](u)".to_string()));
}

#[test]
fn scan_list() {
    let b = "- item";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "item");
    assert!(a.iter().any(|a| matches!(a, Atom::Text { chrome, .. } if matches!(chrome, BlockChrome::ListBullet { ordered: false, index: 1 , .. }))));
}

#[test]
fn scan_ordered_list_index() {
    let b = "1. one\n2. two";
    let a = scan(b);
    assert!(covers(b, &a));
    assert!(a.iter().any(|a| matches!(a, Atom::Text { chrome, .. } if matches!(chrome, BlockChrome::ListBullet { ordered: true, index: 1 , .. }))));
    assert!(a.iter().any(|a| matches!(a, Atom::Text { chrome, .. } if matches!(chrome, BlockChrome::ListBullet { ordered: true, index: 2 , .. }))));
}

#[test]
fn scan_quote() {
    let b = "> q";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "q");
    assert!(a.iter().any(|a| matches!(a, Atom::Text { chrome, .. } if *chrome == BlockChrome::Quote(1))));
}

#[test]
fn scan_fence() {
    let b = "```rust\ncode\n```";
    let a = scan(b);
    assert!(covers(b, &a));
    // The fence lines are hidden; "code" is visible with Code chrome.
    assert_eq!(visible(b, &a), "code");
    assert!(a.iter().any(|a| matches!(a, Atom::Text { chrome, style, .. } if *chrome == BlockChrome::Code && style.code)));
}

#[test]
fn scan_no_inline_in_fence() {
    let b = "```\n**x**\n```";
    let a = scan(b);
    assert!(covers(b, &a));
    // Inside the fence, `**x**` is literal: the `**` are NOT hidden.
    assert_eq!(visible(b, &a), "**x**");
}

#[test]
fn scan_mixed_paragraph() {
    let b = "**b** and `c` and [l](u)";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "b and c and l");
}

#[test]
fn scan_empty() {
    let a = scan("");
    assert!(a.is_empty());
}

#[test]
fn scan_thematic_break() {
    let b = "---";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "");
}

#[test]
fn emphasis_spans_soft_line_break() {
    let b = "*one\ntwo*";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "onetwo");
    assert!(a.iter().all(|a| !matches!(a, Atom::Text { style, .. } if !style.italic)), "both lines italic");
    // The newline is still its own hidden atom (lines paint separately).
    assert!(a.iter().any(|a| matches!(a, Atom::Hidden { range } if &b[range.clone()] == "\n")));
}

#[test]
fn setext_headings() {
    let b = "Title\n=====\nSub\n---\n\nbody";
    let a = scan(b);
    assert!(covers(b, &a));
    assert_eq!(visible(b, &a), "TitleSubbody");
    assert!(a.iter().any(|a| matches!(a, Atom::Text { range, chrome: BlockChrome::Heading(1), .. } if &b[range.clone()] == "Title")));
    assert!(a.iter().any(|a| matches!(a, Atom::Text { range, chrome: BlockChrome::Heading(2), .. } if &b[range.clone()] == "Sub")));
}
