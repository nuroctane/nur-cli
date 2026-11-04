---
name: show-me-the-bug
description: Diagnose a problem from a shown bundle instead of a description: a screen recording, video, screenshots, or annotated images. Use when the user shows a bug, a crash, or a broken UI instead of describing it, or says "show me" / "look at this" with media attached.
---

# Show me the bug

The user shows the problem instead of describing it. Do not ask them to
describe it in prose first. Watch what they showed, extract what matters,
and turn it into a reproduction the agent can act on.

## What you accept

- A short video or screen recording (`.mp4`, `.mov`, `.webm`), a set of
  screenshots, or annotated images. Paths in the prompt auto-attach when
  present; if the user only says they recorded something, ask for the file,
  not for a description.
- A Scrawl-style bundle (screen, voice, clicks, drawings) if one is given:
  treat the recording as the primary source, the clicks and drawings as
  annotations on it, and the voice as narration, not as a transcript to
  quote at length.

## Method

1. **Extract, do not watch frame by frame.** Run `extract_frames` on a
   video (sparse keyframes, default about 1fps, max 8) and `look` at the
   frames and at any screenshots. Note the time or frame where the failure
   first appears, and what was on screen just before it.
2. **Name what happened, in one paragraph.** What the user did, what the
   screen showed, what went wrong, and when. If the failure is not visible
   in the frames, say so and name the missing frame (e.g. "the crash
   itself is not in the recording; the last frame is the loading spinner").
   Do not invent a step the recording does not show.
3. **Reproduce on paper before touching code.** Write the reproduction as:
   setup, steps, expected, actual, and the files or commands that would
   confirm it. Only then orient with `sgrep` and `read_file` for the code
   the reproduction points to.
4. **Rank causes, with evidence.** List at most three candidate causes, each
   with the frame or file:line that supports it and the check that would
   rule it out. Change nothing until the user picks a cause, unless the
   user asked for the fix in the same request.
5. **Hand back a bundle, not a story.** End with: the reproduction, the
   ranked causes, and the next check to run. If the recording was too
   sparse to diagnose, say exactly what to re-record (which screen, which
   action, how long) instead of asking for a better description.

## Scrawl bundles

A Scrawl capture is a `.scrawl` folder, and it is already a bug report:
do not ask the user to re-explain what it shows. Read it in this order:

1. `contact-sheet.png` first, via `look`: annotated keyframes in time
   order, each with its number in a small white chip (the `#N` in
   `summary.md`).
2. `summary.md`: the time-aligned transcript and interaction timeline.
3. `frames/frame-NN.png` via `look` at full size for any keyframe that
   matters (`frame-03.png` is #3). `frames-full/` holds uncropped screens
   when the summary says "Cropped to:".
4. `scrawl.json` for the full manifest: keyframes, marks, events,
   transcript words with times, app context, and autocrop. `recording.mp4`
   / `recording.mov` and `audio.wav` are the source media; `.media-pending`
   means the video is still copying from another machine.

**Marks and clicks vocabulary.** Red strokes, circles, underlines, and
arrows are the problem. `✎①`, `✎②` are marks in drawing order. A ripple
is a click or tap, a finger trail is a swipe, a tapering red trail is a
laser pointer. `⚑` is a deliberately flagged moment. `⟨Name⟩` means the
app reported a new screen. Coordinates are fractions of the captured
area from top-left. Refer back to moments as `#N (M:SS)` so the person
can find them on the contact sheet. Trust the screen over the transcript
when they disagree: speech recognition mishears names and code terms.

**Closer look, only when the `scrawl` CLI is present** (`command -v
scrawl`, else the Scrawl app's binary): `scrawl zoom <bundle> --at 0:21`
or `--at "the width changes"`, `scrawl zoom <bundle> --mark 2`,
`scrawl inspect <bundle> --marks [--json]`,
`scrawl frames <bundle> --from 0:04 --to 0:06 [--fps 4]`,
`scrawl export <bundle> [--out demo.mp4] [--zoom]`. Without the CLI,
read the contact sheet, frames, and `scrawl.json` directly; that is the
whole method, not a fallback. Do not use a `send` command: it targets
other agents' sessions, not nur-cli's. Paste the bundle path and the
contact sheet into this session instead.

**Platform caveat.** Scrawl is Mac-first (macOS 15+, Apple silicon);
the Windows build is an unsigned preview and Linux is not available yet.
On a Windows PC, bundle reading works for captures sent to the user,
but recording and the CLI zoom path cannot be assumed.

## Rules

- Never describe a frame you did not look at. `look` is the vision tool;
  guessing from filenames is not a diagnosis.
- Never frame-by-frame every pixel of a long video. Sparse keyframes first;
  only extract more if the failure window is narrower than the keyframes.
- Voice and drawings are hints, not evidence. A drawn arrow points at a
  thing; the frame still has to show that thing failing.
- Secrets in a recording (tokens, passwords, private data on screen) are
  never quoted, printed, or written into a file. Describe the area, not
  the value, and use the `secret` tool if a credential is needed to
  reproduce the problem.
