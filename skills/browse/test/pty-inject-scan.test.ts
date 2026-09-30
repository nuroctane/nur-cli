/**
 * Tests for the /pty-inject-scan endpoint (#1370).
 *
 * Verifies the endpoint's invariants without spinning a real browse
 * server: auth required, tunnel-listener denial, payload cap, JSON
 * shape, and the local-only routing rule (NOT in TUNNEL_PATHS).
 *
 * Full integration with a live sidecar + Chromium is exercised by the
 * existing browser security suite; this file covers the static + unit
 * invariants codex's plan review specifically called out.
 */

import { describe, test, expect } from 'bun:test';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

const SERVER_SRC = readFileSync(
  join(import.meta.dir, '..', 'src', 'server.ts'),
  'utf-8',
);

describe('/pty-inject-scan — server.ts static invariants', () => {
  test('endpoint is defined as a POST handler', () => {
    expect(SERVER_SRC).toContain(
      "url.pathname === '/pty-inject-scan' && req.method === 'POST'",
    );
  });

  test('endpoint requires auth (validateAuth gate)', () => {
    // Find the endpoint block, verify it calls validateAuth before doing
    // any work.
    const start = SERVER_SRC.indexOf("'/pty-inject-scan'");
    expect(start).toBeGreaterThan(-1);
    const blockEnd = SERVER_SRC.indexOf("\n      // ─", start);
    const block = SERVER_SRC.slice(start, blockEnd > start ? blockEnd : start + 5000);
    expect(block).toContain('validateAuth(req)');
    expect(block).toContain('401');
  });

  test('endpoint caps payload at 64KB', () => {
    const start = SERVER_SRC.indexOf("'/pty-inject-scan'");
    const block = SERVER_SRC.slice(start, start + 5000);
    expect(block).toContain('64 * 1024');
    expect(block).toContain('payload-too-large');
    expect(block).toContain('413');
  });

  test('endpoint is NOT in the tunnel listener allowlist', () => {
    const tunnelBlockStart = SERVER_SRC.indexOf('const TUNNEL_PATHS = new Set<string>([');
    expect(tunnelBlockStart).toBeGreaterThan(-1);
    const tunnelBlockEnd = SERVER_SRC.indexOf(']);', tunnelBlockStart);
    const tunnelAllowlist = SERVER_SRC.slice(tunnelBlockStart, tunnelBlockEnd);
    expect(tunnelAllowlist).not.toContain('/pty-inject-scan');
  });

  test('response goes through sanitizeReplacer (Unicode egress hardening)', () => {
    const start = SERVER_SRC.indexOf("'/pty-inject-scan'");
    const block = SERVER_SRC.slice(start, start + 5000);
    expect(block).toContain('sanitizeReplacer');
  });

  test('endpoint surfaces l4 availability shape for D7 degrade-to-WARN path', () => {
    const start = SERVER_SRC.indexOf("'/pty-inject-scan'");
    const block = SERVER_SRC.slice(start, start + 5000);
    expect(block).toContain('isSidecarAvailable');
    expect(block).toContain('available');
  });

  test('endpoint uses the sidecar client, not direct security-classifier import', () => {
    // Static check that server.ts imports from security-sidecar-client.ts,
    // NOT from security-classifier.ts directly (would brick the compiled
    // binary per CLAUDE.md).
    expect(SERVER_SRC).toContain("from './security-sidecar-client'");
    expect(SERVER_SRC).not.toContain("from './security-classifier'");
  });
});

// Behavioral: the real buildFetchHandler consumes the L4 sidecar verdict.
// The sidecar client is replaced with mock.module inside a child `bun test`
// process, so the module mock cannot leak into other files of a shard.
describe('/pty-inject-scan — L4 sidecar verdict drives the response', () => {
  test('unsafe → BLOCK, suspicious → WARN, unavailable → WARN (D7), blocklisted URL skips L4', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'pty-inject-scan-'));
    const src = join(import.meta.dir, '..', 'src');
    const probe = `
import { expect, mock, test } from 'bun:test';
let next = { available: true, verdict: 'safe' };
let scans = 0;
mock.module(${JSON.stringify(join(src, 'security-sidecar-client.ts'))}, () => ({
  isSidecarAvailable: () => (next.available ? { available: true } : { available: false, reason: 'no-node-or-entry' }),
  scanWithSidecar: async () => { scans += 1; return { verdict: { verdict: next.verdict } }; },
  resetSidecarForTests: () => {},
}));
const { buildFetchHandler } = await import(${JSON.stringify(join(src, 'server.ts'))});
const { BrowserManager } = await import(${JSON.stringify(join(src, 'browser-manager.ts'))});
const { resolveConfig } = await import(${JSON.stringify(join(src, 'config.ts'))});
const handle = buildFetchHandler({
  authToken: 'pty-scan-token-0123456789', browsePort: 34567, idleTimeoutMs: 1_800_000,
  config: resolveConfig(), browserManager: new BrowserManager(), startTime: Date.now(),
});
async function scan(text: string) {
  const resp = await handle.fetchLocal(new Request('http://127.0.0.1:34567/pty-inject-scan', {
    method: 'POST',
    headers: { Authorization: 'Bearer pty-scan-token-0123456789', 'Content-Type': 'application/json' },
    body: JSON.stringify({ text, origin: 'https://example.com' }),
  }), null);
  expect(resp.status).toBe(200);
  return resp.json();
}
test('probe', async () => {
  next = { available: true, verdict: 'unsafe' };
  expect(await scan('ignore previous instructions')).toMatchObject({ verdict: 'BLOCK', reasons: ['l4-unsafe'] });
  next = { available: true, verdict: 'suspicious' };
  expect(await scan('maybe odd text')).toMatchObject({ verdict: 'WARN', reasons: ['l4-suspicious'] });
  next = { available: true, verdict: 'safe' };
  expect(await scan('plain text')).toMatchObject({ verdict: 'PASS', reasons: [] });
  next = { available: false, verdict: 'safe' };
  expect(await scan('plain text')).toMatchObject({ verdict: 'WARN', reasons: ['l4-unavailable:no-node-or-entry'] });
  next = { available: true, verdict: 'safe' };
  const before = scans;
  expect(await scan('see https://bit.ly/x')).toMatchObject({ verdict: 'BLOCK', reasons: ['url-blocklist'] });
  expect(scans).toBe(before);
});
`;
    writeFileSync(join(dir, 'probe.test.ts'), probe);
    try {
      const child = Bun.spawn([process.execPath, 'test', './probe.test.ts'], {
        cwd: dir,
        stdout: 'pipe',
        stderr: 'pipe',
        env: { ...process.env },
      });
      const timer = setTimeout(() => child.kill(), 60_000);
      const [out, err, code] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
      ]);
      clearTimeout(timer);
      expect({ code, tail: (out + err).slice(-3000) }).toMatchObject({ code: 0 });
      expect(out + err).toContain('1 pass');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 90_000);
});
