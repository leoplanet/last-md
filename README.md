# Last-MD

Lightweight WYSIWYG markdown editor in Rust (Typora-clone MVP).
Pure Rust GUI: `eframe`/`egui` + `pulldown-cmark`. Single binary.

## Run

```sh
# debug (fast build)
CARGO_TARGET_DIR=~/.cache/last_md_target cargo run

# release
CARGO_TARGET_DIR=~/.cache/last_md_target cargo run --release
```

`CARGO_TARGET_DIR` is required: the project lives on a cloud-synced drive that
blocks rustc temp files. That drive also stores modification times in whole
seconds, so an edit saved in the same second as a build can be missed —
`touch` the file if a change doesn't seem to take.

Open a file directly: `cargo run -- demo.md` (or Ctrl+O).

## Test

```sh
CARGO_TARGET_DIR=~/.cache/last_md_target cargo test
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

## Keys

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

## Layout

- `src/md.rs` — markdown model: block classification, inline spans,
  smart-input rules. Pure logic, unit-tested.
- `src/main.rs` — egui GUI: WYSIWYG rendering, block editing, source mode,
  find, autosave, status bar.
