# Proposal: inline terminal images, Hermes-style

Status: implemented in v0.68.0 (proposed 2026-11-07). Author: CIA agent. Research: FBI field
agent, from the Hermes Agent repo at tag `v0.23.0` (commit `2bd378e`,
released Nov 5, 2026) and the repo's own vision code.

**As built** (`src/terminal_image.rs`; docs in
[vision.md](../vision.md#inline-images)). Where the build differs from the
plan below:

- One engine, two painters, but the pixel protocols are not hand-rolled a
  second time: kitty, sixel and iTerm2 come from `ratatui-image` (already the
  F4 peek's encoder; sixel through its `icy_sixel`), and half-blocks are the
  engine's own. No new dependencies.
- The TUI draws every inline picture as half-block rows first: ordinary text,
  so the transcript measures, scrolls, selects and re-lays it out like any
  row. Real pixels are drawn over those rows only for kitty and sixel, and
  only while the picture is wholly on screen; their cells are rewritten as
  text when it scrolls. iTerm2 images live on a layer of their own and would
  linger, so they stay in the peek (and in `nur run`).
- Decoding never runs on the UI thread: a background worker fills a 48-entry
  cache and bumps a revision the transcript folds into its re-wrap check.
  Rows are reserved from the header (EXIF orientation applied); 20 MB and
  40 MP caps apply before any decode.
- Tool images: a successful `look`, `browser` / `terminal_browser` /
  `desktop` screenshot, `egaki`, `extract_frames`, `tldraw` / `excalidraw` /
  `penecho` / `debug` export draws its local image under the compact tool
  row; paths come from the call's arguments and result and must exist.
- `nur run` draws the same images on stderr (stdout stays the answer), text
  format and a terminal only: half-blocks one row per write, kitty
  placeholders as text, sixel/iTerm2 over reserved rows (`ESC 7`, up, draw,
  `ESC 8`).
- Detection (§9 answered on the PC): Orca sets `ORCA_PANE_KEY` /
  `ORCA_TERMINAL_HANDLE` / `ORCA_TAB_ID` and clears `TERM_PROGRAM`, so Orca is
  told apart and gets half-blocks. Windows Terminal's sixel setting is not
  visible to the environment, so sixel there is pinned
  (`protocol = "sixel"`), never guessed; `nur doctor` says how. VS Code,
  tmux and screen get half-blocks.
- Not done: the VS Code XTSMGRAPHICS probe (§5.5, optional) and live pixel
  checks in kitty/Ghostty/Windows Terminal with sixel on, which need a person
  at those terminals. The half-block path is verified end to end in a real
  ConPTY terminal (`run_release_ui.py inline-image`).

## 1. Goal

Draw pictures inline in the chat - pasted images and tool screenshots
render as real pixels in the transcript, in the TUI and in the classic
CLI - instead of a compact row that only shows pixels after an explicit
peek. Same ladder Hermes ships: kitty, iTerm2, sixel, half-block
Unicode, with the terminal picked by environment and a config override.

Non-goals: video playback in the terminal; image editing; a new image
format (the `image` crate's png/jpeg/gif/webp/bmp stays the set); the
TinyFish backends (separate proposal); changing the vision path (the
model still gets images through `look` / `input_image`).

## 2. How Hermes does it

Hermes Agent (Nous Research) v0.23.0: "When you paste an image, or a
tool takes a screenshot, the CLI and TUI now draw it right in the chat
instead of printing a file path: real pixels in kitty and Ghostty (CLI
and TUI) and in iTerm2, WezTerm and Konsole (CLI), coloured blocks
everywhere else, and full resolution in VS Code's terminal when its
image support is on." PRs
[#147293](https://github.com/NousResearch/hermes-agent/pull/147293)
(@Adolanium, 60 files) and
[#148874](https://github.com/NousResearch/hermes-agent/pull/148874)
(teknium1, 23 files).

Hermes is Python + TypeScript, not Rust, but the design ports cleanly.
All image work funnels through **one engine,
`agent/terminal_image.py` (476 lines)**, which both runtimes share. The
TUI asks the gateway to render via a JSON-RPC method (`image.render`);
the classic CLI calls the same engine in-process. The engine is split
from the painters: one `render_inline_image(path, mode, max_cols,
max_rows)` producing `{mode, cols, rows, color, transmit, placeholder,
cells}`, with the CLI and the TUI as two thin painters on top. That
split is the design to copy.

### 2.1 Protocols

Config knob (default `auto`): `display.inline_images: auto | kitty |
iterm | unicode | off`.

**Kitty graphics protocol** (kitty, Ghostty; CLI and TUI). Two uses of
the same APC escape:

- Direct transmit+display: `ESC _ G f=100,a=T,q=2,c={cols},r={rows} ...
  ESC \` - `f=100` is a PNG payload, `a=T` transmits and displays,
  `q=2` is quiet (suppresses replies that would corrupt the screen),
  `c`/`r` pin the cell box.
- Virtual placement + Unicode placeholders (the inline-image path, both
  runtimes): transmit once as a *virtual* placement (`U=1`,
  `i={image_id}`), then print ordinary-width placeholder cells the
  terminal paints the image underneath. This is what makes it work
  inside a TUI that owns the screen and measures every cell.
  - Transmit: `ESC _ G a=T,U=1,i={id},c={cols},r={rows},f=100,q=2;<base64-png> ESC \`.
  - Placeholder grid: every cell is **U+10EEEE**, and the first cell of
    each row carries a **combining diacritic that encodes the row
    index** (table copied verbatim from kitty's
    `gen/rowcolumn-diacritics.txt`, ~250 entries starting `0x0305,
    0x030D, 0x030E, 0x0310…`). The **foreground color of the
    placeholder text encodes the image id** as a 24-bit hex color
    (`"#%06x" % (id & 0xFFFFFF)`). Re-sending the same `i` replaces the
    image in place; kitty holds one virtual placement per id, so each
    size of a file gets its own id.
  - Chunking: base64 payload split into **≤4096-byte chunks**, each its
    own APC: first chunk carries the control params, `m=1` means more
    chunks follow, `m=0` is the last.
  - Nominal cell size: `8×16` px. The image is snapped to an exact
    cell multiple before transmitting; a frame that is not a
    whole-cell multiple rounds up and clips the bottom row.
  - Transmit at up to **2 px per nominal cell pixel** for sharpness on
    HiDPI cells without shipping a full-resolution photo through the
    tty.

**iTerm2 inline images (OSC 1337)** (iTerm2, WezTerm, Konsole, VS Code;
classic CLI only): `ESC ] 1337 ;
File=inline=1;size={decoded_bytes};preserveAspectRatio=1;width={cols};height={rows}:{base64-png}
BEL`. Two details that matter: `size` is the **decoded byte count**,
not the base64 length (xterm.js drops an image whose `size` is the
base64 length), and `width`/`height` are in **cells**, pinning the
image to a cell box. The classic CLI painter reserves blank rows for
the image, then jumps back up and draws over them: `ESC 7` (save
cursor) + `ESC[{rows-1}A` (up) + `ESC[{indent}C` (right past the
2-space indent) + transmit + `ESC 8` (restore cursor). Text written
over an image cell erases it, which is why the rows must be blank
first. **The TUI never uses iTerm2** - a TUI cannot reserve rows and
repaint over them, so Hermes forces `iterm` back to `auto` when the
painter has no overlays.

**Sixel** (encoder exists, inline images never use it): a hand-rolled
DEC sixel encoder (Pillow has none): DCS `ESC P 0;1;0 q`, raster
attributes, color registers `#idx;2;{r};{g};{b}` on a **0–100 scale**,
≤255 adaptive colors via median-cut quantization, transparent pixels
skipped (alpha ≤ 32), 6-pixel bands, run-length encoding `!{n}{ch}`
for runs longer than 3, `$` band carriage return, `-` next band. But
inline images never resolve to sixel; the pet mascot is its only
consumer. Sixel terminals get half-blocks for inline images.

**Half-block Unicode** (the universal fallback): one terminal row is
two pixel rows. Each cell is `▀` (upper half) or `▄` (lower half) with
truecolor ANSI: both halves opaque → foreground top + background
bottom + `▀`; one half opaque → foreground-only `▀`/`▄` so the other
half stays the terminal background (no black boxes around transparent
edges); both transparent → a space. Alpha threshold is 32.
Downscaling is Lanczos to the target cell grid.

### 2.2 Detection and fallback chain

Rule: **env vars only, never a terminal query that could hang a pipe**,
with exactly one exception (the VS Code probe below).

`detect_terminal_graphics()`, in order:

1. `TERM_PROGRAM == "vscode"` → `"unicode"` (VS Code/Cursor do not
   scrub an inherited `KITTY_WINDOW_ID`/`ITERM_SESSION_ID`; trusting
   those emits a protocol xterm.js cannot show - a blank frame).
2. `KITTY_WINDOW_ID` set, or `"kitty"`/`"ghostty"` in `TERM`, or
   `TERM_PROGRAM == "ghostty"` → `"kitty"`. WezTerm also lands here
   (`TERM_PROGRAM == "wezterm"` or `WEZTERM_PANE` set) because it
   speaks kitty APC - but see the placeholder caveat below.
3. `TERM_PROGRAM == "iterm.app"` or `ITERM_SESSION_ID` set →
   `"iterm"`.
4. `TERM_PROGRAM == "mintty"`, or `"foot"`/`"mlterm"`/`"sixel"` in
   `TERM` → `"sixel"`.
5. Otherwise → `"unicode"`.

`resolve_inline_mode` then applies the config and the painter's
abilities: `off` → off; `unicode` → unicode; pinned `kitty`/`iterm` →
that mode (with `iterm` demoted to `auto` if the painter cannot do
overlays). `auto`: a remote client's own pick, if supplied, gives kitty
or unicode only. Else **tmux (`TMUX`) or GNU screen (`STY`) →
unicode** (multiplexers swallow the APC transmit). Else kitty if the
terminal supports kitty placeholders (kitty detected **and not
WezTerm** - WezTerm speaks kitty APC but renders placeholders as
tofu). Else iterm if the painter supports overlays and iTerm2 images
are supported (iterm detected, or WezTerm, or `KONSOLE_VERSION` set,
or the VS Code probe said yes). Else unicode.

The TUI's detection mirrors this with its own quirks: VS Code or the
dashboard host (xterm.js shows tofu) → unicode; `TMUX`/`STY` →
unicode; **WezTerm → unicode** (placeholder tofu); kitty iff
`KITTY_WINDOW_ID` or `GHOSTTY_RESOURCES_DIR` or `kitty`/`ghostty` in
`TERM` or `TERM_PROGRAM == "ghostty"`. It returns only
`'kitty' | 'unicode'`.

**The one capability query - VS Code.** VS Code draws images only with
`terminal.integrated.enableImages` on, and nothing in the environment
says so. Its image addon is the only part that answers **XTSMGRAPHICS**,
so one query fenced by DA1 tells: write `\x1b[?1;1;0S\x1b[c` (sixel
support query + DA1 `ESC[c`), read stdin in cbreak mode with `select`,
**50 ms deadline**, and check whether the reply contains the sixel
reply `\x1b[?1;0;`. The read ends as soon as the DA1 fence
(`\x1b[?{digits}c`) arrives, and a late reply is drained (extra 50 ms)
while still in cbreak so it cannot reach the prompt as typed text. Run
**once, before the TUI owns the tty**, cached. Skipped over SSH and on
Windows. When the probe says images are off and mode is `auto`, the
CLI prints a one-line "turn on VS Code images" tip on ~20% of
launches.

**Fallback chain, distilled**: kitty placeholders (kitty, Ghostty) →
iTerm2 images over reserved rows (iTerm2, WezTerm, Konsole,
VS-Code-when-enabled; classic CLI only) → truecolor half-blocks
(everything else, including tmux, screen, SSH, the TUI outside
kitty/Ghostty, and sixel terminals). Sixel is detected but never
selected for inline images.

### 2.3 Image flow

**Sources.** User attachments (`/paste`, `/image`, drag-and-drop)
under the user row; tool images under the tool's line - only the local
image a *successful* tool call produced or looked at
(`vision_analyze`, `image_generate`, and `image_path` /
`screenshot_path` in a result or its multimodal `meta`, e.g.
browser/computer-use screenshots). Nothing for a failed call, and
`read_file` shows none. Only local files are drawn; remote URLs are
never fetched for display. Renderable extensions: `.png .jpg .jpeg
.gif .webp .bmp .tiff .tif .ico`.

**Paste path (TUI).** Clipboard image extracted via a helper, format
sniffed by magic bytes (PNG/JPEG/GIF/WebP), **20 MB cap**; a path is
resolved against the session cwd. Attachments become composer tokens;
on submit they travel with the message, with a revision (turn number
for user turns, tool call id for tool images) that tells two pictures
at one path apart and busts the render cache when a file is re-rendered
at the same path.

**TUI render path.** One render request per (path, revision, width):
resolve the mode, then render at `max_cols = min(cols, 64)`,
`max_rows = min(rows, 16)`, and return `{mode, cols, rows, color,
transmit, placeholder}` for kitty or `{mode, cols, rows, cells}` for
half-blocks (cells are RGBA int tuples) or `{mode: off}`. The TUI
**writes `transmit` once, raw, to stdout** and then paints the
placeholder rows in `color` or the half-block grid as ordinary cells -
so the TUI measures and scrolls them like text. Cache: 48 entries
keyed by path + revision + cols; a `placed` map tracks the size the
terminal currently holds per image color, because kitty holds one
virtual placement per id and a different size displaces it. A render
that is `off` is never cached, so a file that appears later still
shows. Until an image mounts, virtual scroll reserves full height
(max rows per image).

**Classic CLI render path.** Resolve the mode with overlays on, cap
width at `min(64, terminal_columns - 3)`, render each image, then print
**one row per write** (a whole truecolor image in a single write
overruns the Windows console write and the tail rows are lost). The
transmit goes out **raw beside its row** and never enters the output
history. Kitty placeholder rows are wrapped so the truecolor carrying
the image id reaches the terminal verbatim (Hermes wraps them in
prompt_toolkit zero-width brackets because its ANSI printer would
otherwise quantize the truecolor to 256 colors); half-block colors go
through that quantization on purpose.

**Decode/resize (shared).** Grid computed from header size first;
JPEG decodes straight at target scale; **40 MP decode cap** enforced
before any pixel is decoded, with decompression-bomb warnings raised
as errors; EXIF orientation applied (90° orientations swap the grid);
thumbnail with Lanczos; transmit at up to **2 px per nominal cell
pixel** for HiDPI sharpness. Kitty image id is stable per file
revision and grid: `0x8000 + crc32("path:mtime_ns:size:cols×rows") %
(0xFFFFFF - 0x8000)`.

**Scrollback/redraw.** Kitty and half-block images are ordinary text
rows, so a resize replay repaints them (kitty reuses the terminal-side
image by id; the transmit is never re-sent, and never enters history
so a replay cannot crop a 1 MB escape into text). iTerm2 images are
pixels over reserved rows: a replay repaints those rows blank, so the
picture is **lost in the redrawn viewport** and stays visible only in
the scrollback above it.

### 2.4 Limits and gotchas

- Size caps: max **64 cols × 16 rows** per image (both runtimes);
  40 MP decode cap; 20 MB pasted-image cap.
- tmux/screen: without `allow-passthrough on`, tmux swallows the APC
  transmit. The classic CLI wraps transmits in the tmux DCS
  passthrough (`ESC P tmux ; {seq with ESC doubled} ESC \`);
  GNU screen gets the plain DCS wrapper. The TUI sends transmits
  unwrapped and simply falls back to half-blocks under multiplexers.
- WezTerm: speaks kitty APC but paints Unicode placeholders as tofu,
  so the TUI forces half-blocks there while the classic CLI prefers
  iTerm2 for it.
- VS Code: images only with `terminal.integrated.enableImages`;
  inherited `KITTY_WINDOW_ID`/`ITERM_SESSION_ID` must not be trusted
  (blank frame). SSH sessions into VS Code get half-blocks (probe
  skipped). xterm.js drops iTerm2 images whose `size` is the base64
  length.
- Windows Terminal: no native inline-image protocol in this design →
  half-blocks. (nur-cli differs here - see §5.4.)
- Kitty id color must reach the terminal verbatim (truecolor) or the
  id does not decode.
- One virtual placement per image id: two sizes of one file on screen
  at once each need their own id/size.
- Performance: transmits are sent once and cached (48-entry LRU);
  re-renders reuse the terminal-side copy by id.

## 3. Current state in nur-cli

The pieces already exist; the behavior does not. All refs are against
v0.66.0 (`a7ae4412`).

**Vision (sending images to the model) - done.** `docs/vision.md`
(153 lines) is the design doc. `src/tools/media.rs`: `look` tool
(attach images/video for vision), `extract_frames` (ffmpeg keyframes),
`save_clipboard_image` (paste → `.nur/media/paste/`,
`MAX_PENDING = 10`), `fit_image_for_vision` (downscale to a 2000 px
max edge, re-encode above 1.5 MB), `queue_user_images`. `src/tools/
browser.rs` and `src/tools/terminal_browser.rs` have a `screenshot`
action that feeds `look` for vision. `src/theme.rs` has
`theme_preview_png` (96×24 gradient PNG for the theme picker).

**Display (showing images to the user) - peek only.** The TUI has
`Cell::Image { path, label, queued }` (`src/tui/app.rs:368`): drafts
live in the composer while queued; sent attachments are **compact
transcript rows with an explicit peek** (`src/tui/ui.rs:3269`
`image_cell_lines` renders `◈ {label} · {path}` plus `click to
peek`). F4 opens a preview and cycles images; Shift+F4 removes;
Esc closes. The preview uses the `image-peek` feature (default on):
`ratatui-image = 11.1.0` + `image = 0.25` (`Cargo.toml`, feature
`image-peek = ["dep:ratatui-image", "dep:image"]`), `ratatui 0.30.2`,
`crossterm 0.29`, `base64 0.23.1` already in the tree.

**Detection - env-based, no sixel branch.**
`src/tui/app.rs:2768` `prepare_image_picker` builds a
`ratatui_image::picker::Picker`: `NUR_IMAGE_QUERY=1` opts into the
full query (`Picker::from_query_stdio`, can block ~1 s); otherwise an
instant picker whose protocol comes from `inferred_image_protocol`
(`src/tui/app.rs:2720`): tmux + `KITTY_WINDOW_ID` → kitty;
`TERM_PROGRAM` containing iTerm/WezTerm/mintty/vscode/Tabby/Hyper/rio/
Bobcat/WarpTerminal, or `LC_TERMINAL` containing iTerm, or tmux +
iterm hint → iTerm2; else halfblocks. `NUR_IMAGE_PROTOCOL` or the
`[theme] protocol` config (`kitty | sixel | iterm2 | halfblocks |
auto`, `inline_images` toggle) can force a protocol. There is **no
sixel branch** in the instant path: sixel is reachable only by an
explicit force or by the opt-in query. `docs/vision.md` says
"Windows Terminal / conhost fall back to unicode halfblocks art".

**What the user actually runs.** Windows Terminal (55% opacity) and
Orca (Electron + xterm.js). Windows Terminal 1.22+ ships **Sixel**
(opt-in via Settings → Profile → Advanced; DA1 advertises `4` only
when the flag is on; soft 10 MP cap) and does **not** support the
kitty graphics protocol. Orca bundles `@xterm/addon-image` with kitty
enabled, so real pixels are possible there - but the addon's Unicode
placeholder support (`U=1`, U+10EEEE) is unimplemented upstream
(xterm.js issue #5711, PR #6198 pending), so a kitty image sent with
placeholders lands at the cursor instead of inside the widget that
asked for it.

## 4. The gap

1. **No inline-in-transcript pixels.** Pasted images and tool
   screenshots show as a compact row plus a peek. Hermes draws them
   in the flow. This is the feature.
2. **No classic-CLI inline images.** The non-TUI path prints paths
   and text. Hermes's classic CLI draws pixels (kitty / iTerm2 /
   half-blocks) with the same engine.
3. **No sixel detection.** Windows Terminal 1.22+ does sixel, but the
   instant path never picks it. Hermes detects sixel in env vars but
   never selects it for inline images; nur-cli should go one better
   and actually use it on Windows Terminal, because sixel is the only
   pixel protocol that terminal has.
4. **Orca placeholder gap.** `ratatui-image`'s kitty backend places
   images with Unicode placeholders. Orca's addon-image does not
   implement them, so the F4 peek (and any future inline render) can
   misplace in Orca. This must be verified live and handled, not
   assumed away.
5. **Tool screenshots are vision-only.** `browser` /
   `terminal_browser` screenshots feed `look` for the model but are
   never drawn for the user. Hermes draws a successful tool's
   screenshot under the tool's line.

## 5. Design

Copy Hermes's split: **one engine, two thin painters**. Do not rip out
`ratatui-image`; the TUI peek keeps using it, and the engine reuses
the `image` crate the way Pillow serves Hermes.

### 5.1 New module: `terminal_image`

No new dependencies (`image`, `base64` already in the tree). One
public entry point, mirroring Hermes's `render_inline_image`:

```rust
/// Result of rendering one image for inline display.
pub struct InlineImage {
    pub mode: InlineMode, // Kitty | Iterm2 | Halfblocks | Off
    pub cols: u16,
    pub rows: u16,
    /// Kitty virtual-placement id color ("#rrggbb"), when mode is Kitty.
    pub color: Option<String>,
    /// Raw escape bytes to write once to stdout (kitty transmit / iterm2
    /// image). Never stored in scrollback history.
    pub transmit: Option<Vec<u8>>,
    /// Placeholder rows (U+10EEEE grid) for the kitty path.
    pub placeholder: Vec<String>,
    /// Half-block cells: (top_rgba, bottom_rgba) per cell, row-major.
    pub cells: Vec<([u8; 4], [u8; 4])>,
}

pub fn render_inline_image(
    path: &Path,
    mode: InlineMode,
    max_cols: u16,
    max_rows: u16,
) -> Option<InlineImage>;
```

- Decode/resize: `image::load_from_memory`, grid from header size,
  EXIF orientation applied (the `image` crate does not apply EXIF by
  default - read the orientation tag and rotate, 90° swaps the grid),
  Lanczos downscale to the target cell grid, **40 MP decode cap**
  enforced from the header dimensions before decoding, transmit at up
  to 2 px per nominal cell pixel. Nominal cell `8×16` px, snap to an
  exact cell multiple. Caps: `max_cols = min(cols, 64)`,
  `max_rows = min(rows, 16)`, pasted images 20 MB.
- Kitty id: stable per file revision and grid, `0x8000 +
  crc32("path:mtime_ns:size:cols×rows") % (0xFFFFFF - 0x8000)`.
  Copy the U+10EEEE + diacritics placeholder table verbatim from
  kitty's `gen/rowcolumn-diacritics.txt`; the fg color of the
  placeholder text is the id as `"#%06x"`.
- Kitty chunking: base64 payload in **≤4096-byte chunks**, first chunk
  carries the control params, `m=1` more follow, `m=0` last.
- iTerm2: `size` is the **decoded byte count**, `width`/`height` in
  cells. Classic CLI only.
- Sixel: implement the encoder (the `image` crate has none) for the
  Windows Terminal path only - DCS, color registers on a 0–100 scale,
  ≤255 median-cut colors, alpha ≤ 32 skipped, 6-pixel bands, RLE
  `!{n}{ch}` for runs over 3. Reuse it nowhere else.
- Half-blocks: `▀`/`▄` with truecolor ANSI, alpha threshold 32,
  one half opaque → foreground-only so the other half stays the
  terminal background.
- Cache: 48-entry LRU keyed by path + revision + cols (revision is the
  file mtime/size, or the tool call id for tool images). A render that
  is `Off` is never cached.

### 5.2 Detection: extend `inferred_image_protocol`

Keep the env-only rule (no terminal query in the default path). Add,
in order, after the existing kitty/iterm branches:

- `TERM_PROGRAM == "vscode"` → halfblocks (never trust an inherited
  `KITTY_WINDOW_ID` / `ITERM_SESSION_ID` there).
- `TERM_PROGRAM == "WindowsTerminal"` → sixel **when the terminal
  advertises it**: with `NUR_IMAGE_QUERY=1`, the existing
  `Picker::from_query_stdio` path already probes (DA1); without the
  query, sixel is only selected if the user forces it (`protocol =
  "sixel"` or `NUR_IMAGE_PROTOCOL=sixel`). Do not guess: Windows
  Terminal's sixel setting is opt-in, and DA1 advertises `4` only
  when it is on. Document the setting in `docs/vision.md`.
- tmux (`TMUX`) or screen (`STY`) → halfblocks, unless the user pins
  a protocol (kitty pinned + tmux `allow-passthrough on` is the
  escape hatch; wrap transmits in the tmux DCS passthrough,
  `ESC P tmux ; {seq with ESC doubled} ESC \`, screen gets the plain
  DCS wrapper).
- WezTerm → iTerm2 for the classic CLI, halfblocks for the TUI
  (placeholder tofu).

The existing `NUR_IMAGE_PROTOCOL` / `[theme] protocol` override and
`NUR_IMAGE_QUERY=1` opt-in stay exactly as they are.

### 5.3 TUI: inline transcript images

- `Cell::Image` gains an inline render: when the cell is not queued
  and `inline_images` is on, the transcript renders the image at up
  to 16 rows high (Hermes's cap) using the existing picker and a
  `ratatui-image` `StatefulImage`, with the `◈ {label} · {path}` row
  above it and `click to peek` still offered for full size. Queued
  (unsent) attachments stay a compact row until sent.
- The transmit is written raw once and never stored in the cell
  history (a replay must not crop a 1 MB escape into text).
- Tool screenshots: after a `browser` / `terminal_browser`
  `screenshot` action succeeds, attach the PNG as a `Cell::Image`
  under the tool's line (same path as a pasted image). Only local
  files from successful calls; nothing for a failed call; remote URLs
  are never fetched for display.
- **Orca handling (verify live, §9):** if the peek or the inline
  render misplaces in Orca, force halfblocks when the terminal is
  Orca's xterm.js (detect via the environment Orca exposes - confirm
  what `TERM_PROGRAM` / `KITTY_WINDOW_ID` it sets) until upstream
  xterm.js ships placeholder support. Do not ship a TUI that draws
  pictures in the wrong place.

### 5.4 Classic CLI: inline images

- New `print_inline_images(paths)` (Hermes's
  `hermes_cli/cli_inline_images.py:80` shape): resolve the mode with
  overlays on, cap width at `min(64, terminal_columns - 3)`, render
  each image through `render_inline_image`, then print **one row per
  write** (a whole truecolor image in a single write overruns the
  Windows console write and the tail rows are lost). The transmit goes
  out raw beside its row and never enters output history. Kitty
  placeholder rows must reach the terminal with their truecolor id
  verbatim - do not let any ANSI quantization step touch them;
  half-block colors go through quantization on purpose.
- iTerm2 path: reserve blank rows, then `ESC 7` + `ESC[{rows-1}A` +
  `ESC[{indent}C` + transmit + `ESC 8`. Document the redraw loss:
  a resize repaints those rows blank, so the picture survives only in
  scrollback above the viewport.

### 5.5 VS Code probe (optional, later)

The XTSMGRAPHICS + DA1 probe (50 ms deadline, DA1 fence, cbreak
drain, run once before the TUI owns the tty, cached, skipped over
SSH and on Windows) is the only terminal query in the design, and it
exists only because VS Code exposes no env var for
`terminal.integrated.enableImages`. Implement it only if the VS Code
terminal matters to the user; otherwise the `TERM_PROGRAM ==
"vscode"` → halfblocks rule covers it with zero risk of a stray reply
landing as typed text.

## 6. Tests

- Unit: kitty chunking (a 10 KB payload splits into ≤4096-byte
  chunks, `m=1`/`m=0` correct); placeholder grid (row diacritics
  encode the row index, fg color is the id as `#rrggbb`); iTerm2
  `size` is the decoded byte count, not the base64 length; sixel
  encoder round-trips a 2×2 image (registers, bands, RLE);
  half-blocks (transparent half stays the background, alpha ≤ 32);
  decode cap (a header claiming >40 MP is refused before decoding);
  EXIF orientation (a 90° image swaps the grid); id stability (same
  path + mtime + grid → same id).
- Detection: table test over env-var sets (kitty, ghostty, wezterm,
  iterm, vscode, WindowsTerminal, tmux, screen) mirroring the
  existing `image_protocol_preserves_tmux_outer_terminal_precedence`
  test in `src/tui/app.rs`.
- TUI: a `Cell::Image` renders inline at ≤16 rows and the peek still
  opens full-size; a queued attachment stays a compact row.
- E2E (existing scenario shape): paste an image in the TUI → the
  transcript shows the inline render; `browser screenshot` → the PNG
  appears under the tool line; classic CLI `look` on an image prints
  the inline render (or half-blocks when the terminal is unknown).

## 7. Acceptance criteria

- [ ] Pasted image draws inline in the TUI transcript (≤16 rows),
      peek still opens full-size.
- [ ] Tool screenshot draws inline under the tool's line.
- [ ] Classic CLI draws pixels (kitty / iTerm2 / half-blocks) for
      `look` and pasted images.
- [ ] Windows Terminal 1.22+ with sixel on gets real pixels via
      sixel (forced or probed); with sixel off it gets half-blocks.
- [ ] Orca: verified live; if placeholders misplace, the TUI falls
      back to half-blocks there instead of drawing in the wrong place.
- [ ] tmux/screen without passthrough get half-blocks, never a
      swallowed transmit.
- [ ] `cargo check`, `cargo clippy -- -D warnings`, full test suite,
      `python3 scripts/check_docs.py` all clean. No new dependencies.

## 8. Out of scope

- Video playback in the terminal; image editing; new image formats.
- The VS Code probe unless §5.5 is explicitly ordered.
- Changing the vision path (`look` / `input_image` / `extract_frames`
  are untouched).
- Sixel anywhere except the Windows Terminal path (Hermes never uses
  it for inline images; nur-cli uses it only where it is the only
  pixel protocol the terminal has).

## 9. Open questions (verify on the PC before implementing)

1. What environment does Orca's terminal expose (`TERM_PROGRAM`,
   `KITTY_WINDOW_ID`, anything Orca-specific)? That decides the
   Orca detection branch.
2. Does the F4 peek currently place correctly in Orca, or does it
   already misplace (xterm.js #5711)? Screenshot it before changing
   anything.
3. What Windows Terminal version is installed, and is the sixel
   setting on? That decides whether the sixel branch is live or
   forced-only.
4. Does `ratatui-image` 11 expose the font size the picker probed
   (for the 8×16 nominal cell), or should the engine assume 8×16?
