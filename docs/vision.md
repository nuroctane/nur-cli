# Vision

Native multimodal support: send images and short video to the model.

## Overview

NurCLI attaches workspace media on the Responses-style multimodal path (`input_image` /
`input_video`) when the active provider supports it. The model can **see** workspace images
and short video clips directly.

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
