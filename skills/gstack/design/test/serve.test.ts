/**
 * Legacy single-process board server (`$D compare --serve --no-daemon`).
 *
 * Runs the real `serve()` from design/src/serve.ts in a child process on an
 * ephemeral port (port 0), because serve() never returns and exits the
 * process on submit. The daemon owns the default path (daemon.test.ts); this
 * file proves the escape hatch still serves, confines /api/reload to the
 * board directory, and exits 0 after writing feedback.json on submit.
 */

import { afterAll, describe, expect, test } from "bun:test";
import fs from "fs";
import os from "os";
import path from "path";

const SERVE_MODULE = path.resolve(import.meta.dir, "../src/serve.ts");

interface RunningServe {
  proc: ReturnType<typeof Bun.spawn>;
  base: string;
  dir: string;
  html: string;
}

const running: RunningServe[] = [];

async function startServe(): Promise<RunningServe> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "design-serve-"));
  const html = path.join(dir, "board.html");
  fs.writeFileSync(html, "<html><body>BOARD_V1</body></html>");
  const binDir = path.join(dir, "bin");
  fs.mkdirSync(binDir);
  for (const opener of ["xdg-open", "open"]) {
    fs.writeFileSync(path.join(binDir, opener), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  }
  const proc = Bun.spawn(
    [process.execPath, "-e", `import { serve } from ${JSON.stringify(SERVE_MODULE)}; await serve({ html: ${JSON.stringify(html)}, port: 0, timeout: 60 });`],
    {
      env: { ...process.env, PATH: `${binDir}${path.delimiter}${process.env.PATH ?? ""}` },
      stdout: "pipe",
      stderr: "pipe",
    },
  );
  const reader = proc.stderr.getReader();
  const decoder = new TextDecoder();
  let seen = "";
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const { value, done } = await reader.read();
    if (done) break;
    seen += decoder.decode(value);
    const match = /SERVE_STARTED: port=(\d+)/.exec(seen);
    if (match) {
      reader.releaseLock();
      const handle = { proc, base: `http://127.0.0.1:${match[1]}`, dir, html };
      running.push(handle);
      return handle;
    }
  }
  proc.kill();
  throw new Error(`serve() never reported SERVE_STARTED:\n${seen}`);
}

afterAll(() => {
  for (const { proc, dir } of running) {
    proc.kill();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("design serve() (legacy --no-daemon path)", () => {
  test("serves the board, confines /api/reload to the board dir, and exits 0 on submit", async () => {
    const s = await startServe();

    const page = await fetch(`${s.base}/`);
    expect(page.status).toBe(200);
    expect(await page.text()).toContain("BOARD_V1");
    expect(await (await fetch(`${s.base}/api/progress`)).json()).toEqual({ status: "serving" });

    const outside = path.join(os.tmpdir(), `design-serve-outside-${process.pid}.html`);
    fs.writeFileSync(outside, "SECRET");
    try {
      const escape = await fetch(`${s.base}/api/reload`, {
        method: "POST",
        body: JSON.stringify({ html: outside }),
      });
      expect(escape.status).toBe(403);
    } finally {
      fs.rmSync(outside, { force: true });
    }
    const dirReload = await fetch(`${s.base}/api/reload`, {
      method: "POST",
      body: JSON.stringify({ html: s.dir }),
    });
    expect(dirReload.status).toBe(403);

    const v2 = path.join(s.dir, "board-v2.html");
    fs.writeFileSync(v2, "<html><body>BOARD_V2</body></html>");
    const reload = await fetch(`${s.base}/api/reload`, { method: "POST", body: JSON.stringify({ html: v2 }) });
    expect(await reload.json()).toEqual({ reloaded: true });
    expect(await (await fetch(`${s.base}/`)).text()).toContain("BOARD_V2");

    const submit = await fetch(`${s.base}/api/feedback`, {
      method: "POST",
      body: JSON.stringify({ regenerated: false, preferred: "A" }),
    });
    expect(await submit.json()).toEqual({ received: true, action: "submitted" });
    expect(await s.proc.exited).toBe(0);
    expect(JSON.parse(fs.readFileSync(path.join(s.dir, "feedback.json"), "utf-8"))).toEqual({
      regenerated: false,
      preferred: "A",
    });
  });

  test("a second server in the same process binds its own ephemeral port", async () => {
    const a = await startServe();
    const b = await startServe();
    expect(a.base).not.toBe(b.base);
    expect((await fetch(`${a.base}/`)).status).toBe(200);
    expect((await fetch(`${b.base}/`)).status).toBe(200);
  });
});
