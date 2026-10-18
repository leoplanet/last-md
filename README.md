# LAST MD

A lightweight, Typora-style WYSIWYG markdown editor for Linux, written in
Rust. You edit the markdown file directly; formatting renders in place and
the markers (`**`, `#`, `[]()`) show only around the caret. Single binary,
autosaves.

## Build

```sh
cargo build --release
# binary: ./target/release/last_md
```

While developing, `./run.sh` builds (debug) and runs. It uses
`CARGO_TARGET_DIR=~/.cache/last_md_target` because the author's checkout
lives on a cloud-synced drive that blocks rustc temp files; that drive also
stores modification times in whole seconds, so `touch` a file if an edit
doesn't seem to take.

## Install (Arch / Omarchy)

One-liner (downloads the prebuilt binary, no sudo, no build):

```sh
curl -fsSL https://raw.githubusercontent.com/leoplanet/last-md/master/install.sh | sh
```

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/leoplanet/last-md/master/uninstall.sh | sh
```

## Usage

```sh
last-md notes.md   # or just last-md, then Ctrl+O / Ctrl+S
```

| Key | Action |
|-----|--------|
| Ctrl+S / Ctrl+O / Ctrl+N | save (asks for a path if untitled) / open / new |
| Ctrl+C / Ctrl+X / Ctrl+V | copy / cut / paste (markdown source) |
| Ctrl+Insert / Shift+Delete / Shift+Insert, Ctrl+Shift+V | copy / cut / paste (Linux-style; Omarchy Super+C/V/X send Ctrl+C/V/X) |
| Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z | undo / redo |
| Ctrl+A | select all |
| Ctrl+B / Ctrl+I / Ctrl+U | bold / italic / underline (toggle; word at caret if no selection) |
| Ctrl+Shift+` / Alt+Shift+5 | inline code / strikethrough |
| Ctrl+K / Ctrl+Shift+K | link (caret lands in the url) / code fence |
| Ctrl+1…6 / Ctrl+0 | heading level / paragraph |
| Ctrl+Shift+Q / Ctrl+Shift+] / Ctrl+Shift+[ | quote / bullet list / numbered list |
| Ctrl+←/→, Ctrl+Backspace/Delete | move / delete by word |
| Home/End, Ctrl+Home/End, PgUp/PgDn | line, document, page (add Shift to select) |
| Double / triple click | select word / line |
| Ctrl+F | find (Enter next, Shift+Enter previous, Esc close) |
| Ctrl+/ | source mode |
| F8 / Ctrl+D | focus mode / light-dark theme |
| Enter | new block; continues lists; empty list item exits |
| Tab / Shift+Tab | indent / outdent list; next / previous table cell |
| ``` + Enter | code block; leave with ↓, → at end, Ctrl+Enter, or Enter on an empty last line |
| Click a task box | toggle it |

## Tests

```sh
cargo test
```

Two layers:
- `src/md.rs` — unit tests for the markdown model (blocks, styles, smart input).
- `tests/headless.rs` — **headless GUI tests**: drives the real app through
  real egui frames with synthetic input (typing, Enter, arrows, clicks) and
  asserts on state *and* rendered text. This is where caret/focus/interaction
  bugs get caught — add a test here for any editing-behavior fix.
- `tests/alignment.rs` — measures the left edge of every markdown construct
  (run with `--nocapture` for the table) and checks they line up.
- `tests/gallery.rs` — every markdown construct (`tests/fixtures/gallery*.md`) rendered to
  PNGs, and a char-by-char check of inline styling against pulldown-cmark
  (`cargo run --example cmark_html FILE` prints the reference HTML).
- `tests/shortcuts.rs` — clipboard, shortcuts, word/line selection, scrolling,
  find, tables.
- `tests/caret_visual.rs` — caret-placement probes: assert where the caret is
  *painted* and save PNGs to `$CARGO_TARGET_DIR/shots/` for eyeballing.

## Layout

- `src/wysiwyg.rs` — scanner: classifies every buffer byte as hidden marker
  or styled visible text (inline styling via pulldown-cmark).
- `src/layout.rs` — paints the document, lists, quotes, code blocks and
  tables; maps caret/mouse to buffer offsets.
- `src/app.rs` — editor state, keyboard/mouse, clipboard, find, autosave.
- `src/md.rs` — styles and smart-input rules; `src/offset.rs` — caret stops.

## License

MIT, see [LICENSE](LICENSE). The bundled fonts in `fonts/` keep their own
SIL Open Font License (Inter, JetBrains Mono).
