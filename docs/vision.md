# Vision

Native multimodal support: send images and short video to the model - and see
them **inline in the TUI** via your terminal's graphics protocol.

## Overview

NurCLI attaches workspace media on the Responses-style multimodal path (`input_image` /
`input_video`) when the active provider supports it. The model can **see** workspace images
and short video clips directly.

Since v0.28, pasted and attached images also render **inline in the transcript**
using the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/),
sixel, or iTerm2 inline images (via `ratatui-image`), with a halfblocks fallback on
text-only terminals.

---

## Inline image display

| Input | What happens |
|-------|--------------|
| **Ctrl+V an image** (screenshot, copied file) | Saved under `.nur/media/paste/`, rendered inline in the transcript, queued for vision |
| **`/image <path>`** | Renders the workspace image inline + queues it for vision |
| **`look` tool call** | Click the tool card to peek the full-size render |
| **LaTeX equations** | Rendered to PNG and shown inline in answer cards (existing behavior) |

Protocol selection: `auto` by default. Force one via `config.toml`:

```toml
[theme]
protocol = "kitty"   # kitty | sixel | iterm2 | halfblocks | auto
inline_images = true # set false to disable all inline pixel rendering
```

Or per-launch env: `NUR_IMAGE_PROTOCOL=kitty nur`. The full capability probe
(kitty/sixel query + font size) is opt-in because it can block ~1 s on some
terminals: `NUR_IMAGE_QUERY=1 nur`.

!!! note "Terminal support"
    Real pixels need a terminal that speaks one of the protocols - kitty,
    WezTerm, iTerm2, foot, Konsole, mintty, VS Code integrated terminal, rio.
    Windows Terminal / conhost fall back to unicode halfblocks art; everything
    else behaves exactly as before.

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
