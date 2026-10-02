/* The NurCLI demo storyboard: where the camera looks, what the captions say
   and when. Times hang off the take's scene markers, so a re-recorded take
   keeps its choreography; regions are terminal cells (col, row). With
   /sidegraph open the transcript is columns 0-82 and the graph 83-123. */
window.STORYBOARD = ({ VW, VH, winW, winH, at, take, ease }) => {
  const CAST_AT = 3.3; // video time at which the recording starts playing
  const OUTRO = 6.4;
  const span = (t, a, b) => Math.min(1, Math.max(0, (t - a) / (b - a)));
  const lerp = (a, b, p) => a + (b - a) * p;
  const cast = name => {
    const found = take.scenes.find(s => s.name === name);
    if (!found) throw new Error(`take has no scene "${name}"`);
    return found.t;
  };

  /* Dead air plays faster: [from, to, rate] in take seconds. Everything the
     viewer reads (approvals, diffs, answers, modals) stays real time. */
  const fast = [
    [cast('turn') + 0.5, cast('approve-bash') - 0.25, 2.4],
    [cast('reads') + 0.35, cast('approve-edit') - 0.3, 1.7],
    [cast('verify') + 0.25, cast('verify') + 1.3, 1.6],
    [cast('second-turn') + 0.4, cast('approve-test') - 0.25, 2.2],
  ];
  const anchors = [[CAST_AT, 0]];
  let video = CAST_AT;
  let at0 = 0;
  for (const [from, to, rate] of fast) {
    video += from - at0;
    anchors.push([video, from]);
    video += (to - from) / rate;
    anchors.push([video, to]);
    at0 = to;
  }
  video += take.duration - at0;
  anchors.push([video, take.duration]);
  const castEnd = video;
  const duration = castEnd + OUTRO;
  const toVideo = c => {
    for (let i = 1; i < anchors.length; i++) {
      const [v0, c0] = anchors[i - 1];
      const [v1, c1] = anchors[i];
      if (c <= c1) return lerp(v0, v1, span(c, c0, c1));
    }
    return castEnd + (c - take.duration);
  };
  const scene = name => toVideo(cast(name));

  /* Framing. Captions sit bottom-left, so a focused region is centred in the
     space above them. */
  const CAPTION_ROOM = 190;
  const fit = (x0, y0, x1, y1, { maxS = 2.1, pad = 70, room = CAPTION_ROOM, rx = 0, ry = 0 } = {}) => {
    const s = Math.min((VW - 2 * pad) / (x1 - x0), (VH - 2 * pad - room) / (y1 - y0), maxS);
    return { x: (x0 + x1) / 2, y: (y0 + y1) / 2 + room / 2 / s, s, rx, ry };
  };
  const cells = (c0, r0, c1, r1, opts) => {
    const a = at(c0, r0);
    const b = at(c1, r1);
    return fit(a.x, a.y, b.x, b.y, opts);
  };
  const wide = fit(0, 0, winW, winH, { pad: 112, room: 0, maxS: 1 });
  const both = (opts = {}) => cells(0, 8, 124, 37, { maxS: 1.25, ...opts });
  // The graph hugs the window's right edge, so it is framed against that edge
  // (with the transcript filling the left) instead of centred on empty sky.
  const edge = (r0, r1, s, { room = 100, rx = 0, ry = 0 } = {}) => {
    const top = at(0, r0).y;
    const bottom = at(0, r1).y;
    return { x: winW + 60 / s - VW / 2 / s, y: (top + bottom) / 2 + room / 2 / s, s, rx, ry };
  };
  const graph = (opts = {}) => edge(1, 31, 1.5, opts);

  const shots = [
    // The banner draws while the camera leans in.
    { t: scene('launch') + 0.3, dur: 2.3, to: cells(0, 0, 66, 12, { room: 0, maxS: 1.85 }), drift: 0.004 },
    // /sidegraph from the palette, then the panel it opens.
    { t: scene('sidegraph') - 0.1, dur: 1.2, to: cells(0, 20, 96, 40, { maxS: 1.5 }) },
    { t: scene('sidegraph') + 2.0, dur: 1.2, to: cells(36, 0, 124, 30, { maxS: 1.5, ry: -5 }) },
    // Down to the composer, then along the prompt as it is typed.
    { t: scene('prompt') + 0.1, dur: 1.2, to: cells(0, 31, 64, 40, { maxS: 1.8 }) },
    { t: scene('prompt') + 2.0, dur: 3.6, to: cells(40, 31, 104, 40, { maxS: 1.8 }) },
    // The turn: transcript and graph side by side, then each in turn.
    { t: scene('turn') + 0.05, dur: 1.2, to: both() },
    { t: scene('approve-bash') - 0.15, dur: 0.9, to: cells(16, 11, 108, 25, { maxS: 1.75 }) },
    { t: scene('reads') + 0.1, dur: 1.2, to: graph({ ry: -4 }) },
    { t: scene('approve-edit') - 0.15, dur: 1.0, to: cells(20, 7, 104, 31, { maxS: 1.6 }) },
    // The busy line's token breakdown shows while the last request runs.
    { t: scene('verify') + 0.1, dur: 0.8, to: cells(0, 22, 100, 37, { maxS: 1.6, room: 280 }) },
    { t: scene('answer') - 0.2, dur: 1.2, to: cells(0, 18, 84, 37, { maxS: 1.6 }) },
    // Follow the pointer to the edit card, then the diff it opens.
    { t: scene('peek') - 1.1, dur: 1.0, to: cells(0, 6, 84, 26, { maxS: 1.6 }) },
    { t: scene('peek') + 0.25, dur: 1.2, to: cells(0, 0, 84, 21, { maxS: 1.6 }) },
    // Drag the graph open, then scroll back through the whole run.
    { t: scene('graph') - 0.3, dur: 1.2, to: edge(0, 38, 1.2) },
    { t: scene('graph') + 2.2, dur: 1.4, to: edge(2, 30, 1.5, { rx: 3, ry: -7 }) },
    { t: scene('graph') + 4.4, dur: 3.6, to: edge(0, 26, 1.6, { rx: 2, ry: 5 }) },
    { t: scene('palette') + 0.05, dur: 1.0, to: cells(0, 11, 92, 39, { maxS: 1.4 }) },
    // The provider vault; a filter shrinks it to the middle, then the list scrolls.
    { t: scene('providers') + 0.15, dur: 1.1, to: cells(22, 0, 104, 20, { maxS: 1.8 }) },
    { t: scene('providers') + 1.9, dur: 0.8, to: cells(20, 14, 106, 25, { maxS: 1.9 }) },
    { t: scene('providers') + 3.5, dur: 0.8, to: cells(22, 0, 104, 20, { maxS: 1.8 }) },
    { t: scene('providers') + 4.4, dur: 2.4, to: cells(22, 20, 104, 39, { maxS: 1.8 }) },
    { t: scene('models') + 0.3, dur: 1.0, to: cells(20, 11, 106, 28, { maxS: 1.8 }) },
    // Themes: the whole window, tilted, orbiting while it repaints.
    { t: scene('themes') + 0.3, dur: 1.6, to: { ...wide, s: wide.s * 1.04, rx: 6, ry: -10 } },
    { t: scene('themes') + 2.0, dur: 7.5, to: { ...wide, s: wide.s * 1.08, rx: 4, ry: 9 } },
    // The follow-up in the theme just kept: the graph builds again.
    { t: scene('second') - 0.2, dur: 1.2, to: cells(0, 31, 74, 40, { maxS: 1.7 }) },
    { t: scene('second-turn') + 0.1, dur: 1.2, to: graph({ ry: -5 }) },
    { t: scene('approve-test') - 0.15, dur: 0.9, to: cells(16, 9, 108, 28, { maxS: 1.7 }) },
    { t: scene('second-answer') - 0.6, dur: 1.4, to: { ...both({ maxS: 1.15 }), rx: 3, ry: -7 } },
    { t: scene('inspector') - 0.2, dur: 1.2, to: edge(0, 24, 1.6) },
    { t: scene('end') - 0.3, dur: 1.4, to: wide },
  ];

  const captions = [
    { t0: scene('sidegraph') + 0.4, t1: scene('prompt') + 0.3, kicker: 'Sidegraph', title: 'Watch it think', sub: '<code>/sidegraph</code> maps every step of the query live, beside the transcript.' },
    { t0: scene('prompt') + 0.6, t1: scene('approve-bash') - 0.15, kicker: 'Agent', title: 'Ask in plain words', sub: 'nur plans, runs real tools and checks its own work.' },
    { t0: scene('approve-bash') + 0.1, t1: scene('reads') + 0.9, kicker: 'Approvals', title: 'Nothing runs behind your back', sub: 'Manual mode asks first: <code>y</code> once, <code>a</code> always, <code>n</code> deny.' },
    { t0: scene('reads') + 1.0, t1: scene('approve-edit') - 0.05, kicker: 'Tools', title: 'Reads run in parallel', sub: 'Read-only calls go out together; anything that writes waits its turn.' },
    { t0: scene('approve-edit') + 0.15, t1: scene('verify') + 0.3, kicker: 'Edits', title: 'Review the diff before it lands', sub: 'Every edit shows its exact change inside the approval.' },
    { t0: scene('verify') + 0.3, t1: scene('answer') - 0.1, kicker: 'Context', title: 'Every token accounted for', sub: 'The busy line breaks down each request: system, tools, dialogue, tool output.' },
    { t0: scene('answer') + 0.1, t1: scene('peek') - 0.2, kicker: 'Proof', title: 'Fixed, then proven', sub: 'The same suite, rerun: 6 of 6 pass.' },
    { t0: scene('graph') + 0.2, t1: scene('palette') - 0.2, kicker: 'Sidegraph', title: 'Drag it open, scroll it back', sub: 'Tool calls fan out beside the reasoning; every step stays one scroll away.' },
    { t0: scene('peek') + 0.3, t1: scene('graph') - 0.1, kicker: 'Transcript', title: 'Click to peek', sub: 'Mouse-native: open any tool card in place.' },
    { t0: scene('palette') + 0.3, t1: scene('providers') + 0.2, kicker: 'Commands', title: 'One palette for everything', sub: 'Type <code>/</code> for every command and 1,000+ skills.' },
    { t0: scene('providers') + 0.5, t1: scene('models') - 0.1, kicker: 'Providers', title: '65 providers, one terminal', sub: 'API keys, OAuth or browser sign-in, plus TypeSafe Jev and Enclave security sidecars.' },
    { t0: scene('models') + 0.4, t1: scene('themes') - 0.1, kicker: 'Models', title: 'Switch models mid-session', sub: "Your provider's live catalog, filtered as you type." },
    { t0: scene('themes') + 0.6, t1: scene('second') - 0.1, kicker: 'Themes', title: '41 themes, previewed live', sub: 'Every surface repaints as you browse; Enter keeps one.' },
    { t0: scene('second') + 0.3, t1: scene('inspector') - 0.3, kicker: 'Sidegraph', title: 'Same graph, new look', sub: 'Synthwave, kept a moment ago. Every view follows the theme.' },
    { t0: scene('inspector') + 0.2, t1: scene('end') + 0.3, kicker: 'Inspector', title: 'See every call', sub: '<code>F6</code> opens changes, tools, agents and context.' },
  ];

  // Pointer visits: the intro click on the app icon, then the take's mouse
  // events, grouped into one trip when they follow each other closely.
  const ICON = { x: VW / 2, y: 448 };
  const CLICK_ICON = 2.0;
  const visits = [{ events: [{ t: CLICK_ICON, button: 'left', screen: ICON }] }];
  for (const click of take.clicks) {
    const event = { t: toVideo(click.t), button: click.button || 'left', world: at(click.x + 0.5, click.y + 0.5) };
    const last = visits[visits.length - 1].events;
    if (last[0].world && event.t - last[last.length - 1].t < 2.0) {
      last.push(event);
    } else {
      visits.push({ events: [event] });
    }
  }

  const OPEN = [CLICK_ICON + 0.12, CLICK_ICON + 1.35];
  const CLOSE = [castEnd + 0.85, castEnd + 1.95];
  const windowAt = (t, cam) => {
    if (t < OPEN[0]) return { opacity: 0, dx: 0, dy: 0, k: 0.1, rx: 0, blur: 0 };
    if (t < CLOSE[0]) {
      const p = ease.outQuart(span(t, OPEN[0], OPEN[1]));
      return {
        opacity: span(t, OPEN[0], OPEN[0] + 0.18),
        dx: 0,
        dy: lerp((ICON.y - VH / 2) / cam.s, 0, p),
        k: lerp(168 / (winW * cam.s), 1, p),
        rx: lerp(18, 0, p),
        blur: lerp(4, 0, p),
      };
    }
    const p = ease.inOut(span(t, CLOSE[0], CLOSE[1]));
    return { opacity: 1 - p, dx: 0, dy: lerp(0, 60, p), k: lerp(1, 0.86, p), rx: lerp(0, -10, p), blur: lerp(0, 9, p) };
  };

  // The title card's text leaves on the click; the icon stays until the
  // window it opens has covered it.
  const titleCardAt = t => {
    const inP = ease.out(span(t, 0.35, 1.25));
    const textOut = ease.inOut(span(t, CLICK_ICON + 0.02, CLICK_ICON + 0.4));
    const iconOut = ease.inOut(span(t, CLICK_ICON + 0.2, CLICK_ICON + 0.75));
    const press = 1 - 0.06 * Math.sin(Math.PI * span(t, CLICK_ICON, CLICK_ICON + 0.22));
    return {
      opacity: inP,
      y: (1 - inP) * 24,
      k: lerp(0.965, 1, inP),
      blur: (1 - inP) * 12,
      text: { opacity: 1 - textOut, y: textOut * 18, blur: textOut * 8 },
      icon: { opacity: 1 - iconOut, k: press * lerp(1, 1.18, iconOut), blur: iconOut * 6 },
    };
  };
  const endCardAt = t => {
    const inP = ease.out(span(t, CLOSE[0] + 0.5, CLOSE[1] + 0.75));
    return { opacity: inP, y: (1 - inP) * 26, k: lerp(0.96, 1, inP), blur: (1 - inP) * 12 };
  };
  const glowAt = t => span(t, 0.3, 1.8) * (1 - 0.4 * span(t, CLOSE[0], CLOSE[1]));
  const fadeAt = t => Math.max(1 - span(t, 0, 0.5), span(t, duration - 0.45, duration));

  return {
    anchors,
    scenes: Object.fromEntries(take.scenes.map(s => [s.name, +toVideo(s.t).toFixed(3)])),
    duration,
    initial: wide,
    shots,
    captions,
    visits,
    windowAt,
    titleCardAt,
    endCardAt,
    glowAt,
    fadeAt,
    // README teaser: the diff approval, the theme orbit, the graph in Synthwave.
    teaser: [
      [scene('approve-edit') + 0.2, scene('approve-edit') + 3.4],
      [scene('themes') + 1.2, scene('themes') + 7.4],
      [scene('second-turn') + 0.6, scene('second-answer') + 1.2],
    ].map(([a, b]) => [+a.toFixed(3), +b.toFixed(3)]),
  };
};
