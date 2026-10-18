//! Print the CommonMark HTML of a file (reference for visual checks).
fn main() {
    let src = std::fs::read_to_string(std::env::args().nth(1).expect("path")).unwrap();
    let opts = pulldown_cmark::Options::all();
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new_ext(&src, opts));
    print!("{html}");
}
