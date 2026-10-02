# Demo film

The NurCLI demo is recorded from the real binary, then filmed. Every frame of
the terminal is nur's own output; only the model is a stand-in, so each take is
identical.

1. **Record** (Windows, needs `pyte`):

   ```bash
   python scripts/demo/record.py
   ```

   Drives `target/release/nur.exe` in a ConPTY against `demo_provider.py`, a
   scripted Responses server, inside a small `calc` project with a real
   precedence bug at `C:\code\calc` (removed afterwards). The failing run, the
   reads, the diff and the passing run are genuine tool output. Writes
   `target/demo/take.cast` (asciicast v2) and `take.json` (scene markers, key
   presses and clicks).

2. **Check the take**: `python scripts/demo/snapshot.py prompt+2 themes+4`
   prints the screen at scene markers.

3. **Preview and render** (Node 20+, ffmpeg, Playwright's Chromium):

   ```bash
   cd scripts/demo && npm install
   node render.mjs --serve        # live preview with a scrub bar
   node render.mjs --stills 12,30 # PNG stills in target/demo/stills
   node render.mjs                # master + variants in target/demo
   ```

   The render supersamples at 1.5x and writes `nur-demo-master.mp4`,
   `nur-demo.mp4` (1080p60, for posting), `nur-demo-web.mp4` (900p30, for
   the site) and the README teaser `nur-demo.webp`. `--poster <take seconds>
   --scale 2` renders the site's poster and `--card <take seconds> --scale 2`
   the share-card plate, each one clean frame of the take.

`player/storyboard.js` hangs every camera move, caption and speed-up off the
take's scene markers, so a re-recorded take keeps its choreography. The page
draws each frame from the video time alone, which keeps renders deterministic.
