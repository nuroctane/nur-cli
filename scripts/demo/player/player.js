/* NurCLI demo player.

   Replays a recorded asciicast in xterm.js inside a macOS-style window and
   films it: a keyframed camera, captions, keycaps and a pointer. Every pixel
   is a pure function of the video time, so a render is deterministic:
   render.mjs calls player.renderAt(t) once per frame and screenshots the
   stage. Opened without ?render the page plays in real time with a scrub bar;
   ?t=12.5 freezes on one moment. */
(() => {
  'use strict';

  const VW = 1920;
  const VH = 1080;
  const query = new URLSearchParams(location.search);
  const RENDER = query.has('render');

  const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const span = (t, a, b) => (b <= a ? (t >= b ? 1 : 0) : clamp((t - a) / (b - a)));
  const ease = {
    linear: t => t,
    in: t => t * t * t,
    out: t => 1 - Math.pow(1 - t, 3),
    outQuart: t => 1 - Math.pow(1 - t, 4),
    outExpo: t => (t >= 1 ? 1 : 1 - Math.pow(2, -10 * t)),
    inOut: t => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2),
    inOutQuint: t => (t < 0.5 ? 16 * t ** 5 : 1 - Math.pow(-2 * t + 2, 5) / 2),
    smooth: t => t * t * t * (t * (6 * t - 15) + 10),
    outBack: t => 1 + 2.2 * Math.pow(t - 1, 3) + 1.2 * Math.pow(t - 1, 2),
  };

  function rng(seed) {
    return () => {
      seed = (seed + 0x6d2b79f5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  const $ = id => document.getElementById(id);

  /* ── Data ──────────────────────────────────────────────────────────── */
  async function loadTake() {
    const name = query.get('take') || 'take';
    const [castText, take] = await Promise.all([
      fetch(`/data/${name}.cast`).then(r => r.text()),
      fetch(`/data/${name}.json`).then(r => r.json()),
    ]);
    const lines = castText.split('\n').filter(Boolean);
    const header = JSON.parse(lines[0]);
    const events = [];
    for (const line of lines.slice(1)) {
      const [t, kind, data] = JSON.parse(line);
      if (kind === 'o') events.push([t, data]);
    }
    return { header, events, take };
  }

  /* Piecewise-linear video→cast map through [video, cast] anchors. */
  function timeMap(anchors) {
    const pick = (list, value, from, to) => {
      if (value <= list[0][from]) return list[0][to] + (value - list[0][from]);
      for (let i = 1; i < list.length; i++) {
        const a = list[i - 1];
        const b = list[i];
        if (value <= b[from]) return lerp(a[to], b[to], span(value, a[from], b[from]));
      }
      const last = list[list.length - 1];
      return last[to] + (value - last[from]);
    };
    return {
      cast: v => Math.max(0, pick(anchors, v, 0, 1)),
      video: c => pick(anchors, c, 1, 0),
    };
  }

  /* ── Terminal ──────────────────────────────────────────────────────── */
  function makeTerminal(cols, rows) {
    const term = new Terminal({
      cols,
      rows,
      allowProposedApi: true,
      fontFamily: '"JetBrains Mono", "Cascadia Mono", "Segoe UI Symbol", "Segoe UI Emoji", monospace',
      fontSize: 18,
      lineHeight: 1.0,
      letterSpacing: 0,
      cursorBlink: false,
      cursorStyle: 'bar',
      cursorInactiveStyle: 'bar',
      cursorWidth: 2,
      scrollback: 0,
      disableStdin: true,
      drawBoldTextInBrightColors: false,
      theme: {
        background: '#0b0e12',
        foreground: '#ece6d6',
        cursor: '#d8c494',
        cursorAccent: '#0b0e12',
        selectionBackground: 'rgba(216,196,148,0.25)',
      },
    });
    term.loadAddon(new Unicode11Addon.Unicode11Addon());
    term.unicode.activeVersion = '11';
    return term;
  }

  /* Feeds the cast into the terminal up to a cast time. Seeking backwards
     replays from the start, which is exact and fast enough for preview. */
  class Feeder {
    constructor(term, events) {
      this.term = term;
      this.events = events;
      this.index = 0;
      this.time = -1;
    }
    async seek(t) {
      if (t < this.time) {
        this.term.reset();
        this.index = 0;
      }
      let chunk = '';
      while (this.index < this.events.length && this.events[this.index][0] <= t) {
        chunk += this.events[this.index][1];
        this.index++;
      }
      this.time = t;
      if (chunk) await new Promise(resolve => this.term.write(chunk, resolve));
    }
  }

  function sampleBackground(term, fallback) {
    const buffer = term.buffer.active;
    const counts = new Map();
    const probes = [[0, 0], [0, 60], [1, 2], [20, 1], [20, 110], [term.rows - 1, 1]];
    for (const [y, x] of probes) {
      const line = buffer.getLine(buffer.viewportY + y);
      const cell = line && line.getCell(x);
      if (!cell || !cell.isBgRGB()) continue;
      const value = cell.getBgColor();
      counts.set(value, (counts.get(value) || 0) + 1);
    }
    let best = null;
    let bestCount = 0;
    for (const [value, count] of counts) {
      if (count > bestCount) {
        best = value;
        bestCount = count;
      }
    }
    if (best === null) return fallback;
    return [(best >> 16) & 255, (best >> 8) & 255, best & 255];
  }

  /* ── Backdrop: drifting dithered particle ribbons, as on nuroctane.xyz ─ */
  function makeBackdrop(canvas, dpr) {
    canvas.width = VW * dpr;
    canvas.height = VH * dpr;
    const ctx = canvas.getContext('2d');
    const rand = rng(1337);
    const gauss = () => {
      let u = 0;
      let v = 0;
      while (u === 0) u = rand();
      while (v === 0) v = rand();
      return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
    };
    // Each ribbon is a cubic Bezier the particles stream along.
    const ribbons = [
      { p: [[-260, 1180], [420, 820], [980, 1080], [2180, 260]], width: 96, count: 5200, speed: 0.012 },
      { p: [[-200, 380], [520, 120], [1300, 700], [2160, -160]], width: 70, count: 2800, speed: 0.009 },
      { p: [[1500, 1240], [1700, 900], [1820, 600], [2200, 540]], width: 54, count: 1300, speed: 0.015 },
    ];
    const bez = (p, u) => {
      const a = 1 - u;
      const b0 = a * a * a;
      const b1 = 3 * a * a * u;
      const b2 = 3 * a * u * u;
      const b3 = u * u * u;
      return [
        b0 * p[0][0] + b1 * p[1][0] + b2 * p[2][0] + b3 * p[3][0],
        b0 * p[0][1] + b1 * p[1][1] + b2 * p[2][1] + b3 * p[3][1],
      ];
    };
    const particles = [];
    for (const [r, ribbon] of ribbons.entries()) {
      for (let i = 0; i < ribbon.count; i++) {
        particles.push({
          r,
          u: rand(),
          v: gauss() * ribbon.width,
          speed: ribbon.speed * (0.6 + rand() * 0.8),
          wobble: rand() * Math.PI * 2,
          size: rand() < 0.85 ? 2 : 3,
          alpha: 0.1 + Math.pow(rand(), 2) * 0.7,
          gold: rand() < 0.07,
        });
      }
    }
    const stars = [];
    for (let i = 0; i < 380; i++) {
      stars.push({ x: rand() * VW, y: rand() * VH, a: 0.05 + rand() * 0.22, phase: rand() * 6.28, size: rand() < 0.9 ? 2 : 3 });
    }

    return (t, cam, base) => {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const g = ctx.createRadialGradient(VW * 0.5, VH * 0.46, 80, VW * 0.5, VH * 0.5, VW * 0.72);
      g.addColorStop(0, '#0d0d0e');
      g.addColorStop(0.55, '#070707');
      g.addColorStop(1, '#020202');
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, VW, VH);

      // Parallax: the field moves a fraction of the camera, so pans read as depth.
      const zoom = 1 + (cam.s / base.s - 1) * 0.07;
      const ox = -(cam.x - base.x) * cam.s * 0.05;
      const oy = -(cam.y - base.y) * cam.s * 0.05;
      ctx.translate(VW / 2 + ox, VH / 2 + oy);
      ctx.scale(zoom, zoom);
      ctx.translate(-VW / 2, -VH / 2);

      for (const star of stars) {
        const a = star.a * (0.6 + 0.4 * Math.sin(t * 0.9 + star.phase));
        ctx.fillStyle = `rgba(244,241,234,${a.toFixed(3)})`;
        ctx.fillRect(Math.round(star.x / 2) * 2, Math.round(star.y / 2) * 2, star.size, star.size);
      }
      for (const p of particles) {
        const ribbon = ribbons[p.r];
        const u = (p.u + p.speed * t) % 1;
        const [x0, y0] = bez(ribbon.p, u);
        const [x1, y1] = bez(ribbon.p, Math.min(1, u + 0.002));
        let nx = -(y1 - y0);
        let ny = x1 - x0;
        const len = Math.hypot(nx, ny) || 1;
        nx /= len;
        ny /= len;
        const off = p.v * (1 + 0.18 * Math.sin(t * 0.35 + p.wobble + u * 9));
        const x = x0 + nx * off;
        const y = y0 + ny * off;
        const edge = Math.min(1, u * 6, (1 - u) * 6);
        const a = p.alpha * edge;
        if (a < 0.01) continue;
        ctx.fillStyle = p.gold ? `rgba(216,196,148,${(a * 1.2).toFixed(3)})` : `rgba(244,241,234,${a.toFixed(3)})`;
        ctx.fillRect(Math.round(x / 2) * 2, Math.round(y / 2) * 2, p.size, p.size);
      }
      ctx.setTransform(1, 0, 0, 1, 0, 0);
    };
  }

  function makeGrain() {
    const c = document.createElement('canvas');
    c.width = 256;
    c.height = 256;
    const ctx = c.getContext('2d');
    const img = ctx.createImageData(256, 256);
    const rand = rng(99);
    for (let i = 0; i < img.data.length; i += 4) {
      const v = Math.floor(rand() * 255);
      img.data[i] = v;
      img.data[i + 1] = v;
      img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
    return c.toDataURL('image/png');
  }

  /* ── Camera ────────────────────────────────────────────────────────── */
  function makeCamera(initial, shots) {
    const ordered = shots.slice().sort((a, b) => a.t - b.t);
    const settle = (shot, from, t) => {
      const p = (shot.ease || ease.inOut)(span(t, shot.t, shot.t + shot.dur));
      const to = { ...from, ...shot.to };
      const s = Math.exp(lerp(Math.log(from.s), Math.log(to.s), p));
      const held = Math.max(0, t - shot.t - shot.dur);
      return {
        x: lerp(from.x, to.x, p),
        y: lerp(from.y, to.y, p),
        s: s * (1 + (shot.drift || 0) * held),
        rx: lerp(from.rx, to.rx, p),
        ry: lerp(from.ry, to.ry, p),
      };
    };
    // Each shot starts from wherever the previous one had got to when it
    // began, so overlapping moves blend instead of jumping.
    return t => {
      let cam = { ...initial };
      for (let i = 0; i < ordered.length; i++) {
        const shot = ordered[i];
        if (t < shot.t) break;
        const next = ordered[i + 1];
        const until = next && next.t <= t ? next.t : t;
        cam = settle(shot, cam, until);
        if (until === t) break;
      }
      return cam;
    };
  }

  /* ── Overlays ──────────────────────────────────────────────────────── */
  function makeCaptions(list) {
    const host = $('captions');
    return list.map(item => {
      const el = document.createElement('div');
      el.className = 'cap';
      el.innerHTML = `${item.kicker ? `<div class="kicker">${item.kicker}</div>` : ''}<div class="title">${item.title}</div>${item.sub ? `<div class="sub">${item.sub}</div>` : ''}`;
      host.appendChild(el);
      return { ...item, el };
    });
  }

  function drawCaptions(caps, t) {
    for (const cap of caps) {
      const inP = ease.out(span(t, cap.t0, cap.t0 + 0.55));
      const outP = ease.in(span(t, cap.t1 - 0.4, cap.t1));
      const vis = inP * (1 - outP);
      if (vis <= 0.001) {
        cap.el.style.opacity = '0';
        cap.el.style.display = 'none';
        continue;
      }
      cap.el.style.display = '';
      cap.el.style.opacity = vis.toFixed(4);
      const y = (1 - inP) * 22 - outP * 10;
      const blur = (1 - inP) * 10 + outP * 6;
      cap.el.style.transform = `translate3d(0, ${y.toFixed(2)}px, 0)`;
      cap.el.style.filter = blur > 0.05 ? `blur(${blur.toFixed(2)}px)` : 'none';
    }
  }

  function makeKeys(keys, toVideo) {
    const groups = [];
    for (const key of keys) {
      const t = toVideo(key.t);
      const last = groups[groups.length - 1];
      if (last && last.label === key.label && last.hint === key.hint && t - last.times[last.times.length - 1] < 1.0) {
        last.times.push(t);
      } else {
        groups.push({ label: key.label, hint: key.hint, times: [t] });
      }
    }
    const host = $('keys');
    for (const group of groups) {
      const el = document.createElement('div');
      el.className = 'key';
      el.innerHTML = `<span class="k">${group.label}</span>${group.hint ? `<span class="hint">${group.hint}</span>` : ''}<span class="count" hidden></span>`;
      el.style.display = 'none';
      host.appendChild(el);
      group.el = el;
      group.count = el.querySelector('.count');
    }
    return groups;
  }

  function drawKeys(groups, t) {
    for (const group of groups) {
      const first = group.times[0];
      const last = group.times[group.times.length - 1];
      const inP = ease.outBack(span(t, first, first + 0.2));
      const outP = ease.in(span(t, last + 0.85, last + 1.15));
      if (t < first || outP >= 1) {
        group.el.style.display = 'none';
        continue;
      }
      group.el.style.display = '';
      let pressed = 0;
      let bump = 0;
      for (const at of group.times) {
        if (at <= t) {
          pressed++;
          bump = Math.max(bump, 1 - span(t, at, at + 0.12));
        }
      }
      const scale = (0.6 + 0.4 * inP) * (1 - 0.07 * bump);
      group.el.style.opacity = (clamp(inP * 1.4) * (1 - outP)).toFixed(4);
      group.el.style.transform = `translate3d(0, ${((1 - inP) * 16 + outP * 8).toFixed(2)}px, 0) scale(${scale.toFixed(4)})`;
      if (pressed > 1) {
        group.count.hidden = false;
        group.count.textContent = `×${pressed}`;
      } else {
        group.count.hidden = true;
      }
    }
  }

  /* ?raw: the bare take on a still camera, for checking a recording that
     the storyboard does not cover (a failed or partial take). */
  function rawBoard({ winW, winH }, take) {
    const s = Math.min((VW - 116) / winW, (VH - 116) / winH, 1);
    const hidden = { opacity: 0, y: 0, k: 1, blur: 0 };
    return {
      anchors: [[0, 0], [take.duration, take.duration]],
      scenes: Object.fromEntries(take.scenes.map(x => [x.name, x.t])),
      duration: take.duration,
      initial: { x: winW / 2, y: winH / 2, s, rx: 0, ry: 0 },
      shots: [],
      captions: [],
      visits: [],
      windowAt: () => ({ opacity: 1, dx: 0, dy: 0, k: 1, rx: 0, blur: 0 }),
      titleCardAt: () => ({ ...hidden, text: hidden, icon: hidden }),
      endCardAt: () => hidden,
      glowAt: () => 1,
      fadeAt: () => 0,
      teaser: [],
    };
  }

  /* ?card=<take seconds>: one moment of the take as a share-card plate, the
     window tilted into the right of the frame and nothing over it, so the
     card renderer can set type on the left. */
  function cardBoard(geometry, take, castT) {
    const board = rawBoard(geometry, take);
    const { winW, winH } = geometry;
    return {
      ...board,
      anchors: [[0, castT], [1, castT + 1]],
      // The share card sets type on the left; the poster is the window alone.
      initial: query.get('layout') === 'poster'
        ? { x: winW * 0.5, y: winH * 0.5, s: 0.9, rx: 5, ry: -12 }
        : { x: winW * 0.06, y: winH * 0.5, s: 0.78, rx: 4, ry: -18 },
      noKeys: true,
    };
  }

  /* ── Boot ──────────────────────────────────────────────────────────── */
  async function boot() {
    const { header, events, take } = await loadTake();
    await Promise.all([
      document.fonts.load('18px "JetBrains Mono"'),
      document.fonts.load('bold 18px "JetBrains Mono"'),
    ]);

    const term = makeTerminal(header.width, header.height);
    term.open($('term'));
    const screen = $('term').querySelector('.xterm-screen');
    const cell = { w: screen.offsetWidth / header.width, h: screen.offsetHeight / header.height };
    const win = $('window');
    const termPad = { x: 14, top: 42 + 10 };
    const winW = win.offsetWidth;
    const winH = win.offsetHeight;
    win.style.width = `${winW}px`;

    const geometry = {
      VW, VH, winW, winH, cell, cols: header.width, rows: header.height,
      // World position of a terminal cell's top-left corner.
      at: (col, row) => ({ x: termPad.x + col * cell.w, y: termPad.top + row * cell.h }),
    };
    const board = query.has('card')
      ? cardBoard(geometry, take, Number(query.get('card')))
      : query.has('raw')
        ? rawBoard(geometry, take)
        : window.STORYBOARD({ ...geometry, take, ease });
    const map = timeMap(board.anchors);
    const camera = makeCamera(board.initial, board.shots);
    const caps = makeCaptions(board.captions);
    const keys = board.noKeys ? [] : makeKeys(take.keys, map.video);
    const visits = board.visits;
    const feeder = new Feeder(term, events);
    const dpr = window.devicePixelRatio || 1;
    const drawBackdrop = makeBackdrop($('backdrop'), dpr);
    $('grain').style.backgroundImage = `url(${makeGrain()})`;
    const duration = board.duration;

    const world = $('world');
    const pointer = $('pointer');
    const ripple = pointer.querySelector('.ripple');
    const glow = $('glow');
    const titleCard = $('title-card');
    const titleText = titleCard.querySelector('.title-text');
    const appIcon = $('app-icon');
    const endCard = $('end-card');
    const fade = $('fade');
    let lastBg = '';

    // A visit is the pointer's trip for a run of events close in time (a
    // click, wheel ticks, a drag): it glides in, follows the events, leaves.
    function drawPointer(t, cam) {
      const toScreen = p => p.screen || {
        x: VW / 2 + (p.world.x - cam.x) * cam.s,
        y: VH / 2 + (p.world.y - cam.y) * cam.s,
      };
      for (const visit of visits) {
        const events = visit.events;
        const first = events[0];
        const appear = first.t - 1.15;
        const leave = events[events.length - 1].t + 0.9;
        if (t < appear || t > leave + 0.3) continue;
        // On screen and above everything; cell targets follow the camera.
        const start = toScreen(first);
        let x;
        let y;
        if (t < first.t) {
          const travel = ease.inOut(span(t, appear + 0.05, first.t - 0.12));
          x = lerp(start.x + 190, start.x, travel);
          y = lerp(start.y + 150, start.y, travel);
        } else {
          ({ x, y } = start);
          for (let i = 1; i < events.length; i++) {
            const a = toScreen(events[i - 1]);
            const b = toScreen(events[i]);
            if (t < events[i].t) {
              const p = span(t, events[i - 1].t, events[i].t);
              x = lerp(a.x, b.x, p);
              y = lerp(a.y, b.y, p);
              break;
            }
            ({ x, y } = b);
          }
        }
        let down = 0;
        let ring = -1;
        let held = false;
        for (const event of events) {
          if (t < event.t) break;
          if (event.button === 'press') {
            held = true;
            ring = span(t, event.t, event.t + 0.55);
          } else if (event.button === 'release') {
            held = false;
            down = Math.max(down, 1 - span(t, event.t, event.t + 0.16));
          } else if (event.button === 'wheel') {
            down = Math.max(down, 0.4 * (1 - span(t, event.t, event.t + 0.1)));
          } else if (event.button !== 'move') {
            down = Math.max(down, 1 - span(t, event.t, event.t + 0.16));
            ring = span(t, event.t, event.t + 0.55);
          }
        }
        if (held) down = 1;
        const opacity = span(t, appear, appear + 0.2) * (1 - span(t, leave, leave + 0.3));
        pointer.style.opacity = opacity.toFixed(4);
        pointer.style.transform = `translate3d(${(x - 7).toFixed(2)}px, ${(y - 3.5).toFixed(2)}px, 0) scale(${(1.3 * (1 - 0.14 * down)).toFixed(4)})`;
        ripple.style.opacity = ring >= 0 ? (0.95 * (1 - ring)).toFixed(4) : '0';
        ripple.style.transform = `scale(${(1 + ease.out(Math.max(ring, 0)) * 6).toFixed(3)})`;
        return;
      }
      pointer.style.opacity = '0';
    }

    async function renderAt(t) {
      await feeder.seek(map.cast(t));

      const cam = camera(t);
      const open = board.windowAt(t, cam);
      world.style.transform =
        `translate3d(${VW / 2}px, ${VH / 2}px, 0) rotateX(${cam.rx.toFixed(3)}deg) rotateY(${cam.ry.toFixed(3)}deg) ` +
        `scale(${cam.s.toFixed(5)}) translate3d(${(-cam.x).toFixed(3)}px, ${(-cam.y).toFixed(3)}px, 0)`;
      win.style.opacity = open.opacity.toFixed(4);
      win.style.transform = `translate3d(${open.dx.toFixed(2)}px, ${open.dy.toFixed(2)}px, 0) rotateX(${open.rx.toFixed(3)}deg) scale(${open.k.toFixed(5)})`;
      win.style.filter = open.blur > 0.05 ? `blur(${open.blur.toFixed(2)}px)` : 'none';

      // The chrome follows the theme: sample the painted background.
      const bg = sampleBackground(term, [11, 14, 18]);
      const key = bg.join(',');
      if (key !== lastBg) {
        lastBg = key;
        const [r, g, b] = bg;
        win.style.setProperty('--win-bg', `rgb(${r},${g},${b})`);
        const light = 0.2126 * r + 0.7152 * g + 0.0722 * b > 140;
        win.style.setProperty('--win-title', light ? 'rgba(20,18,14,0.55)' : 'rgba(244,241,234,0.55)');
        win.style.setProperty('--win-title-hi', light ? 'rgba(20,18,14,0.8)' : 'rgba(244,241,234,0.8)');
      }

      drawBackdrop(t, cam, board.initial);
      const centre = { x: VW / 2 + (winW / 2 - cam.x) * cam.s, y: VH / 2 + (winH / 2 - cam.y) * cam.s };
      glow.style.transform = `translate3d(${(centre.x - 960).toFixed(1)}px, ${(centre.y - 540).toFixed(1)}px, 0) scale(${(0.9 + 0.1 * Math.sin(t * 0.6)).toFixed(4)})`;
      glow.style.opacity = (board.glowAt(t)).toFixed(4);

      const title = board.titleCardAt(t);
      titleCard.style.opacity = title.opacity.toFixed(4);
      titleCard.style.transform = `translate3d(0, ${title.y.toFixed(2)}px, 0) scale(${title.k.toFixed(4)})`;
      titleCard.style.filter = title.blur > 0.05 ? `blur(${title.blur.toFixed(2)}px)` : 'none';
      titleText.style.opacity = title.text.opacity.toFixed(4);
      titleText.style.transform = `translate3d(0, ${title.text.y.toFixed(2)}px, 0)`;
      titleText.style.filter = title.text.blur > 0.05 ? `blur(${title.text.blur.toFixed(2)}px)` : 'none';
      appIcon.style.opacity = title.icon.opacity.toFixed(4);
      appIcon.style.transform = `scale(${title.icon.k.toFixed(4)})`;
      appIcon.style.filter = title.icon.blur > 0.05 ? `blur(${title.icon.blur.toFixed(2)}px)` : 'none';
      const end = board.endCardAt(t);
      endCard.style.opacity = end.opacity.toFixed(4);
      endCard.style.transform = `translate3d(0, ${end.y.toFixed(2)}px, 0) scale(${end.k.toFixed(4)})`;
      endCard.style.filter = end.blur > 0.05 ? `blur(${end.blur.toFixed(2)}px)` : 'none';
      fade.style.opacity = board.fadeAt(t).toFixed(4);

      drawPointer(t, cam);
      drawCaptions(caps, t);
      drawKeys(keys, t);

      // Let xterm paint the rows it just parsed before the frame is taken.
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    }

    window.player = { ready: true, duration, renderAt, geometry, map, board };
    if (RENDER) return;

    const scrub = $('scrub');
    const seek = $('seek');
    const clock = $('clock');
    const play = $('play');
    scrub.hidden = false;
    seek.max = String(duration);
    let playing = !query.has('t');
    let origin = performance.now() / 1000 - Number(query.get('t') || 0);
    let busy = false;
    let current = Number(query.get('t') || 0);
    play.textContent = playing ? 'pause' : 'play';
    play.onclick = () => {
      playing = !playing;
      play.textContent = playing ? 'pause' : 'play';
      origin = performance.now() / 1000 - current;
    };
    seek.oninput = () => {
      current = Number(seek.value);
      origin = performance.now() / 1000 - current;
    };
    const tick = async () => {
      if (!busy) {
        busy = true;
        if (playing) current = (performance.now() / 1000 - origin) % duration;
        await renderAt(current);
        seek.value = String(current);
        clock.textContent = current.toFixed(2);
        busy = false;
      }
      requestAnimationFrame(tick);
    };
    tick();
  }

  window.player = { ready: false };
  boot().catch(error => {
    window.player = { ready: false, error: String(error && error.stack || error) };
    document.body.insertAdjacentHTML('beforeend', `<pre style="color:#f66;position:fixed;top:0;left:0">${String(error && error.stack || error)}</pre>`);
  });
})();
