import { describe, test, expect } from 'bun:test';
import * as fs from 'fs';
import * as path from 'path';

// v1.44 Commit 3 — detach state machine + ring buffer + re-attach replay.
//
// The state machine is what turns a single network blip from "fall through
// to ENDED state, click Restart" into "silent re-attach with scrollback
// intact, keep typing." Live WS cycles + buffer-overflow exercises belong
// in the e2e tier; these static-grep tripwires defend the load-bearing
// protocol + correctness properties.

const AGENT_TS = path.resolve(import.meta.path, '..', '..', 'src', 'terminal-agent.ts');

describe('terminal-agent detach + re-attach (v1.44+ Commit 3)', () => {
  test('2. RING_BUFFER_MAX_BYTES default is 1 MB, env-overridable', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    expect(src).toContain('GSTACK_PTY_RING_BUFFER_BYTES');
    expect(src).toContain('1024 * 1024');
  });

  test('3. DETACH_WINDOW_MS default is 60s, env-overridable', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    expect(src).toContain('GSTACK_PTY_DETACH_WINDOW_MS');
    expect(src).toContain("'60000'");
  });

  test('7. WS open() re-attaches when sessionId already lives in sessionsById', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    const block = sliceBetween(src, 'open(ws) {', 'message(ws, raw) {');
    expect(block).toContain('sessionsById.get(sessionId)');
    expect(block).toContain('existing.liveWs = ws');
    expect(block).toContain('clearTimeout(existing.detachTimer)');
    // Tells the client to write RIS before treating the next binary
    // frame as replay.
    expect(block).toContain("type: 'reattach-begin'");
    expect(block).toContain('sendBinary(buildReplayPayload(existing))');
  });

  test('8. WS close starts detach timer for non-intentional close codes', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    const i = src.indexOf('close(ws');
    const j = src.indexOf('function handleTabState', i);
    const block = src.slice(i, j);
    // 4001 = intentional restart (Commit 2), 4404 = no-claude, 1000 = clean
    // exit. Any other code (1006 abnormal, 1001 going-away, etc.) gets the
    // 60s detach grace.
    expect(block).toContain('code === 4001');
    expect(block).toContain('code === 4404');
    expect(block).toContain('code === 1000');
    expect(block).toContain('session.detached = true');
    expect(block).toContain('session.detachTimer = setTimeout');
    expect(block).toContain('DETACH_WINDOW_MS');
    // Detach timer must unref so the bun process can exit cleanly.
    expect(block).toContain('detachTimer as any)?.unref?.()');
  });

  test('9. /internal/restart cancels detach timer before disposal', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    const block = sliceBetween(src, "url.pathname === '/internal/restart'", "// /claude-available");
    // Without the cancellation, a later detach-timer fire would dispose a
    // session that's already been disposed by the explicit restart path.
    expect(block).toContain('clearTimeout(session.detachTimer)');
  });

  test('10. PTY on-data writes through session.liveWs (not the original ws closure)', () => {
    const src = fs.readFileSync(AGENT_TS, 'utf-8');
    // Critical for re-attach correctness: the PTY's on-data callback
    // closes over `session`, not the original `ws`, so after re-attach
    // it routes to the new liveWs automatically.
    expect(src).toContain('session.liveWs.sendBinary');
    // Always append to the ring buffer regardless of attach state — so
    // a detached session still captures output for the next re-attach.
    expect(src).toContain('appendToRingBuffer(session, flush)');
  });
});

function sliceBetween(source: string, start: string, end: string): string {
  const i = source.indexOf(start);
  if (i === -1) throw new Error(`marker not found: ${start}`);
  const j = source.indexOf(end, i + start.length);
  if (j === -1) throw new Error(`end marker not found: ${end}`);
  return source.slice(i, j);
}
