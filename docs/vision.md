# Vision

Native multimodal support: send images and short video to the model, and see
them **inline in the transcript** and in a full-size preview, using your
terminal's graphics protocol.

## Overview

NurCLI attaches workspace media on the Responses-style multimodal path (`input_image` /
`input_video`) when the active provider supports it. The model can **see** workspace images
and short video clips directly.

Pasted images stay in a compact composer attachment row until you send. F4
opens a preview and cycles through multiple images; Shift+F4 removes the
previewed image (or the last attachment when no preview is open). Esc closes
the preview. Enter can send an image-only message. Once sent, the picture is
drawn in the transcript under its `◈ label · path` row, at most 64 columns by
16 rows, and click-to-peek still opens it full size. Images queued while a turn
runs travel with that follow-up rather than entering the active request.

## Inline images

Pictures are drawn where they happen, not behind a click:

- **Attachments** you paste (Ctrl+V), stage (`/image`) or reference, once sent.
- **Tool images** under the tool's line, for a successful call that produced or
  looked at a local image: `look`, `browser` / `terminal_browser` / `desktop`
  screenshots, `egaki` generations, `extract_frames`, exported `tldraw` /
  `excalidraw` / `penecho` boards. Nothing for a failed call; remote URLs are
  never fetched for display.
- **`nur run`** draws the same tool images on stderr when it is a terminal and
  the output format is text (stdout stays the answer).

Every inline picture is first a block of half-block rows (`▀` with the top
pixel as foreground and the bottom as background): ordinary text, so it
scrolls, wraps and selects like the rest of the transcript in every terminal.
Where the terminal speaks kitty or sixel, real pixels are drawn over those rows
while the picture is wholly on screen. iTerm2-style images sit on a layer of
their own and would linger when the transcript scrolls, so in those terminals
the transcript keeps half-blocks and the peek shows the pixels. `nur run` uses
kitty placeholders, or draws sixel and iTerm2 images over rows it reserves
first.

Decoding happens off the UI thread: the rows are reserved from the file header
(EXIF orientation applied) and fill in when the render lands. Files over 20 MB
or 40 megapixels are refused before any pixel is decoded and say so in one row.
Sixel is sized in pixels, so pinned sixel lines up with its rows best with the real cell size: launch with `NUR_IMAGE_QUERY=1` (one terminal query at startup) when the picture overhangs or falls short. A pinned kitty or sixel still draws half-blocks inline under tmux, screen, Orca, VS Code and WezTerm, where the transmit would be swallowed, misplaced or shown as blanks. `[theme] inline_images = false` turns all of it off; `nur doctor` prints the
mode in use (`images  ...`).

| Terminal | Inline picture |
|----------|----------------|
| kitty, Ghostty | kitty pixels |
| Windows Terminal 1.22+ with Sixel on, foot, mlterm | sixel pixels once pinned: `protocol = "sixel"` or `NUR_IMAGE_PROTOCOL=sixel` (the setting is invisible to the environment, so nur does not guess) |
| iTerm2, WezTerm, Konsole | half-blocks in the TUI, pixels in the peek and in `nur run` |
| VS Code / Cursor, Orca, tmux, GNU screen | half-blocks (an inherited `KITTY_WINDOW_ID`, Orca's xterm.js and multiplexers would show a blank or misplaced frame) |
| everything else | half-blocks |

Previews use kitty, sixel, or iTerm2 graphics with a halfblocks fallback.

---

## Image attachments and previews

| Input | What happens |
|-------|--------------|
| **Ctrl+V an image** (screenshot, copied file) | Saved under `.nur/media/paste/`, staged in the composer, sent for vision with your message |
| **`/image <path>`** | Stages the workspace image in the composer |
| **`look` tool call** | Drawn under the tool card; click it to peek the full-size render |
| **LaTeX equations** | Rendered to PNG and shown inline in answer cards (existing behavior) |

Protocol selection: `auto` by default. Force one via `config.toml`:

```toml
[theme]
protocol = "kitty"   # kitty | sixel | iterm2 | halfblocks | auto
inline_images = true # set false for compact rows only (no inline pictures)
```

Or per-launch env: `NUR_IMAGE_PROTOCOL=kitty nur`. The full capability probe
(kitty/sixel query + font size) is opt-in because it can block ~1 s on some
terminals: `NUR_IMAGE_QUERY=1 nur`.

!!! note "Terminal support"
    The F4 peek draws real pixels in a terminal that speaks one of the
    protocols - kitty, WezTerm, iTerm2, foot, Konsole, mintty, VS Code (with
    `terminal.integrated.enableImages` on), rio. Inline pictures follow the
    table above.
    Windows Terminal 1.22+ draws sixel once it is turned on (Settings > profile
    > Advanced > Sixel) and pinned with `protocol = "sixel"`; otherwise, and in
    conhost, pictures are half-block art.

---

## Tools

### `look`

Attach workspace **image(s)** or a **video** so the model sees them on the next turn.

| Input type | Formats | Notes |
|------------|---------|-------|
| Images | png, jpg, webp, gif | Direct attachment (no ffmpeg needed) |
| Video | mp4, webm, mov | mp4 accepted directly up to ~20 MB; webm/mov go through `extract_frames` |

**Usage in TUI:** The agent calls `look` automatically, or you can reference media paths in your prompt.

!!! note "mp4 is accepted directly"
    Unlike other video formats, `.mp4` files under ~20 MB are sent directly to the model via `input_video` without needing ffmpeg. For webm/mov or larger files, use `extract_frames` first.

### `extract_frames`

Extract sparse **keyframes** from video via **ffmpeg**.

| Setting | Default |
|---------|---------|
| Frame rate | ~1 fps |
| Max frames | ~8 |
| Output | `.nur/frames/<name>/` |

After extraction, `look` is auto-queued with the extracted frames.

---

## Auto-attach

Media paths in your user prompt are **automatically attached** when the file exists in the workspace:

```text
"steal UI design tokens from demo.mp4 and scaffold a matching component"
```

If `demo.mp4` exists in your project, it is automatically sent to the model.

---

## Design from video

A typical workflow for extracting design tokens from a reference clip:

1. **Short video (< 20 MB):** Reference it directly in your prompt
   ```text
   "match the animation in ref.mp4"
   ```

2. **Longer video:** Extract frames first, then reference them
   ```text
   "extract keyframes from walkthrough.mp4 and implement the sidebar"
   ```

3. **Manual control:** The agent will use `extract_frames` → inspect stills → implement using **design-eng** skills

!!! tip "Best practices"
    - Prefer sparse frames over frame-by-frame
    - Longer / huge videos: extract frames first; don't `look` a giant file
    - `extract_frames` requires ffmpeg on PATH (check with `nur doctor`)
    - `look` still works on short videos and images without ffmpeg

---

## Requirements

| Tool | Requires |
|------|----------|
| `look` | Nothing extra for images; ffmpeg optional for short video |
| `extract_frames` | **ffmpeg** on PATH |

Check vision readiness:

```bash
nur doctor
# should show: vision  look · extract_frames (input_image / input_video)
```

Install ffmpeg:

=== "Windows"

    ```powershell
    winget install ffmpeg
    # or
    choco install ffmpeg
    ```

=== "macOS"

    ```bash
    brew install ffmpeg
    ```

=== "Linux"

    ```bash
    sudo apt install ffmpeg
    ```
