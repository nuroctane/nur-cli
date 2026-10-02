// Render the NurCLI demo video from a recorded take (see README.md here).
//
//   node render.mjs --serve [--port 5190]     live preview with a scrub bar
//   node render.mjs --stills 12.5,30,47.2     PNG stills into target/demo/stills
//   node render.mjs [--fps 60] [--scale 1.5]  the master video, then web variants
//   node render.mjs --variants-only            re-encode the variants from the master
//   node render.mjs --poster 37 --scale 2      the site poster from take second 37
//   node render.mjs --card 37 --scale 2        the share-card plate (nuroctane.xyz og.mjs)
//
// The page under player/ draws every frame from the video time alone, so a
// render is deterministic: each frame is renderAt(t), then a screenshot piped
// straight into ffmpeg.
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const OUT = path.join(ROOT, 'target', 'demo');

const FILES = {
  '/': path.join(HERE, 'player', 'index.html'),
  '/assets/logo.png': path.join(ROOT, 'docs', 'assets', 'nur-cli-logo.png'),
};
const DIRS = [
  ['/player/', path.join(HERE, 'player')],
  ['/node_modules/', path.join(HERE, 'node_modules')],
  // JetBrains Mono (OFL) is already vendored with the canvas-design skill.
  ['/fonts/', path.join(ROOT, 'skills', 'canvas-design', 'canvas-fonts')],
  ['/data/', OUT],
];
const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.cast': 'text/plain; charset=utf-8',
  '.ttf': 'font/ttf',
  '.png': 'image/png',
};

function args() {
  // JPEG at q97 from a 1.5x supersample is visually lossless after the
  // downscale, and about ten times faster to capture than PNG.
  const out = { fps: 60, scale: 1.5, port: 0, crf: 16, format: 'jpeg' };
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i++) {
    const name = argv[i].replace(/^--/, '');
    if (name === "serve") out.serve = true;
    else if (name === "info") out.info = true;
    else if (name === "variants-only") out.variantsOnly = true;
    else if (name === "no-gpu") out.noGpu = true;
    else if (name === "take") out.take = argv[++i];
    else if (name === "raw") out.raw = true;
    else if (name === "card") out.card = argv[++i];
    else if (name === "poster") out.poster = argv[++i];
    else if (name === 'no-variants') out.noVariants = true;
    else out[name] = argv[++i];
  }
  for (const key of ['fps', 'scale', 'port', 'crf']) out[key] = Number(out[key]);
  return out;
}

function fileFor(urlPath) {
  if (FILES[urlPath]) return FILES[urlPath];
  for (const [prefix, dir] of DIRS) {
    if (!urlPath.startsWith(prefix)) continue;
    const file = path.resolve(dir, `.${urlPath.slice(prefix.length - 1)}`);
    if (file.startsWith(dir + path.sep)) return file;
  }
  return null;
}

async function serve(port) {
  const server = http.createServer((req, res) => {
    const urlPath = decodeURIComponent(new URL(req.url, 'http://local').pathname);
    const file = fileFor(urlPath);
    if (!file || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      res.writeHead(404);
      res.end('not found');
      return;
    }
    res.writeHead(200, {
      'content-type': TYPES[path.extname(file)] || 'application/octet-stream',
      'cache-control': 'no-store',
    });
    fs.createReadStream(file).pipe(res);
  });
  server.listen(port, '127.0.0.1');
  await once(server, 'listening');
  return server;
}

async function openPlayer(url, scale) {
  const { chromium } = await import('playwright');
  const args = ['--force-color-profile=srgb', '--font-render-hinting=none', '--disable-lcd-text', '--hide-scrollbars'];
  // Headless Chromium composites in software (SwiftShader) by default; the
  // GPU path is several times faster per frame. --no-gpu falls back.
  if (!process.argv.includes('--no-gpu')) {
    args.push('--enable-gpu', process.platform === 'win32' ? '--use-angle=d3d11' : '--use-angle=default');
  }
  const browser = await chromium.launch({ args });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: scale });
  page.on('pageerror', error => console.error(`page error: ${error.message}`));
  page.on('console', message => {
    if (message.type() === 'error') console.error(`console: ${message.text()}`);
  });
  const take = process.argv.includes('--take') ? process.argv[process.argv.indexOf('--take') + 1] : 'take';
  const raw = process.argv.includes('--raw') ? '&raw=1' : '';
  const flag = name => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : null);
  const card = flag('--card') ? `&card=${flag('--card')}` : flag('--poster') ? `&card=${flag('--poster')}&layout=poster` : '';
  await page.goto(`${url}/?render=1&take=${encodeURIComponent(take)}${raw}${card}`);
  await page.waitForFunction(() => window.player && (window.player.ready || window.player.error), null, { timeout: 60_000 });
  const error = await page.evaluate(() => window.player.error);
  if (error) throw new Error(error);
  const duration = await page.evaluate(() => window.player.duration);
  return { browser, page, duration };
}

function ffmpeg(argv) {
  const child = spawn('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...argv], {
    stdio: ['pipe', 'inherit', 'inherit'],
  });
  const done = new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('exit', code => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`))));
  });
  return { child, done };
}

async function renderVideo(page, duration, opts) {
  const fps = opts.fps;
  const from = Number(opts.from ?? 0);
  const to = Math.min(Number(opts.to ?? duration), duration);
  const master = path.join(OUT, opts.out || 'nur-demo-master.mp4');
  const scaleFilter = opts.scale === 1 ? [] : ['-vf', 'scale=1920:1080:flags=lanczos'];
  const encoder = ffmpeg([
    '-f', 'image2pipe', '-framerate', String(fps), '-i', '-',
    ...scaleFilter,
    '-c:v', 'libx264', '-preset', 'slow', '-crf', String(opts.crf), '-tune', 'animation',
    '-pix_fmt', 'yuv420p', '-r', String(fps), '-movflags', '+faststart', master,
  ]);
  const first = Math.round(from * fps);
  const last = Math.round(to * fps);
  const started = Date.now();
  for (let frame = first; frame < last; frame++) {
    await page.evaluate(t => window.player.renderAt(t), frame / fps);
    const shot = await page.screenshot({ type: opts.format, quality: opts.format === 'jpeg' ? 97 : undefined });
    if (!encoder.child.stdin.write(shot)) await once(encoder.child.stdin, 'drain');
    const done = frame - first + 1;
    if (done % fps === 0 || frame === last - 1) {
      const rate = done / ((Date.now() - started) / 1000);
      const eta = (last - frame - 1) / rate;
      process.stdout.write(`\rframe ${done}/${last - first}  ${rate.toFixed(1)} fps  eta ${Math.round(eta)}s   `);
    }
  }
  encoder.child.stdin.end();
  await encoder.done;
  process.stdout.write(`\nwrote ${master}\n`);
  return master;
}

async function variants(master, { teaser }) {
  const jobs = [
    // Social and announcement cut: full quality, 60 fps.
    ['-i', master, '-c:v', 'libx264', '-preset', 'slow', '-crf', '18', '-tune', 'animation',
      '-pix_fmt', 'yuv420p', '-movflags', '+faststart', path.join(OUT, 'nur-demo.mp4')],
    // Web embed: 900p30 stays sharp at page width and lands near 15 MB. The
    // drifting particle field is what costs bits; AV1 and VP9 keep its noise
    // and come out larger, so one H.264 file serves every browser.
    ['-i', master, '-vf', 'fps=30,scale=1600:900:flags=lanczos', '-c:v', 'libx264', '-preset', 'slow',
      '-crf', '29', '-tune', 'animation', '-pix_fmt', 'yuv420p', '-movflags', '+faststart',
      path.join(OUT, 'nur-demo-web.mp4')],
  ];
  // README teaser: a few crossfaded moments as a light animated WebP.
  const FADE = 0.4;
  const parts = teaser.map(([from, to], i) =>
    `[0:v]trim=start=${from}:end=${to},setpts=PTS-STARTPTS,fps=15,scale=960:-2:flags=lanczos[s${i}]`);
  let chain = '[s0]';
  let length = teaser[0][1] - teaser[0][0];
  for (let i = 1; i < teaser.length; i++) {
    const out = i === teaser.length - 1 ? '[teaser]' : `[x${i}]`;
    parts.push(`${chain}[s${i}]xfade=transition=fade:duration=${FADE}:offset=${(length - FADE).toFixed(3)}${out}`);
    length += teaser[i][1] - teaser[i][0] - FADE;
    chain = out;
  }
  jobs.push(['-i', master, '-filter_complex', parts.join(';'), '-map', '[teaser]', '-c:v', 'libwebp_anim',
    '-lossless', '0', '-q:v', '74', '-compression_level', '6', '-loop', '0', path.join(OUT, 'nur-demo.webp')]);
  for (const job of jobs) {
    const run = ffmpeg(job);
    run.child.stdin.end();
    await run.done;
    const file = job[job.length - 1];
    console.log(`wrote ${file} (${(fs.statSync(file).size / 1e6).toFixed(1)} MB)`);
  }
}

async function main() {
  const opts = args();
  const server = await serve(opts.port);
  const url = `http://127.0.0.1:${server.address().port}`;
  if (opts.serve) {
    console.log(`preview: ${url}/   (?t=12.5 freezes a moment)`);
    return;
  }
  fs.mkdirSync(OUT, { recursive: true });
  const { browser, page, duration } = await openPlayer(url, opts.scale);
  try {
    if (opts.info) {
      const info = await page.evaluate(() => {
        const { geometry, board } = window.player;
        return { duration: board.duration, winW: geometry.winW, winH: geometry.winH, cell: geometry.cell, scenes: board.scenes };
      });
      console.log(JSON.stringify(info));
      return;
    }
    if (opts.poster) {
      // The site's poster: one clean frame of the take, no captions on it.
      await page.evaluate(() => window.player.renderAt(0));
      const frame = path.join(OUT, 'poster-frame.png');
      await page.screenshot({ path: frame, type: 'png' });
      const poster = path.join(OUT, 'nur-demo-poster.jpg');
      const run = ffmpeg(['-i', frame, '-vf', 'scale=1920:1080:flags=lanczos', '-q:v', '2', poster]);
      run.child.stdin.end();
      await run.done;
      fs.rmSync(frame);
      console.log(poster);
      return;
    }
    if (opts.card) {
      // The share-card plate: 16:9 at 2x, cropped to the 1.91:1 card shape.
      await page.evaluate(() => window.player.renderAt(0));
      const frame = path.join(OUT, 'og-plate-frame.png');
      await page.screenshot({ path: frame, type: 'png' });
      const plate = path.join(OUT, 'og-plate.jpg');
      const run = ffmpeg(['-i', frame, '-vf', 'crop=iw:iw/1.905,scale=2400:1260:flags=lanczos', '-q:v', '3', plate]);
      run.child.stdin.end();
      await run.done;
      console.log(plate);
      return;
    }
    if (opts.stills) {
      const dir = path.join(OUT, 'stills');
      fs.mkdirSync(dir, { recursive: true });
      for (const t of String(opts.stills).split(',').map(Number)) {
        await page.evaluate(at => window.player.renderAt(at), t);
        const file = path.join(dir, `${t.toFixed(2)}.png`);
        await page.screenshot({ path: file, type: 'png' });
        console.log(file);
      }
      return;
    }
    const board = await page.evaluate(() => ({ teaser: window.player.board.teaser }));
    let master = path.join(OUT, opts.out || 'nur-demo-master.mp4');
    if (!opts.variantsOnly) {
      console.log(`duration ${duration.toFixed(2)}s at ${opts.fps} fps, scale ${opts.scale}`);
      master = await renderVideo(page, duration, opts);
    }
    if (!opts.noVariants) await variants(master, board);
  } finally {
    await browser.close();
    server.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exit(1);
});
