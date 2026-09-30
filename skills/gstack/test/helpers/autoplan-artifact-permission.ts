/** One-time input for a cropped native Edit of an already-owned review artifact. */
import * as fs from 'node:fs';
import * as path from 'node:path';
import type { PendingAutoplanArtifact } from './autoplan-artifact-recorder';
import type { NativePublicToolEvent } from './plan-count-transcript';

interface ArtifactPermissionContext {
  cwd: string;
  /** Set only by the launcher that created HOME/.gstack, never from ambient env. */
  ownedStateRoot?: string;
  /** Optional native plans root supplied by the same isolated launcher. */
  ownedNativePlansRoot?: string;
  commandStartedAt: number;
  now?: number;
  transcriptStatus: string;
  publicTools: NativePublicToolEvent[];
}

const MAX_BYTES = 1024 * 1024;

export function ownedAutoplanArtifact(file: string, context: Pick<ArtifactPermissionContext, 'cwd' | 'ownedStateRoot'>): boolean {
  if (!context.ownedStateRoot || !path.isAbsolute(file) || path.resolve(file) !== file) return false;
  const project = path.join(context.ownedStateRoot, 'projects', path.basename(context.cwd));
  const relative = path.relative(project, file).split(path.sep).join('/');
  // Current CEO plan and both explicit Eng test-plan layouts. Design/DX amend
  // ACTIVE_PLAN; they have no separate state-root plan directory. Do not admit
  // restore points, methodology snapshots, task logs, config, or mockups.
  if (!/^ceo-plans\/\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]*\.md$/.test(relative) &&
      !/^[a-zA-Z0-9][a-zA-Z0-9_.-]*-test-plan-\d{8}-\d{6}\.md$/.test(relative)) return false;
  try {
    // System temp parents may be aliases (/var -> /private/var on macOS).
    // Canonicalize above the owned root; no symlink at or below it is admitted.
    const expected = path.join(fs.realpathSync(context.ownedStateRoot), path.relative(context.ownedStateRoot, file));
    return fs.lstatSync(context.ownedStateRoot).isDirectory() &&
      fs.realpathSync(file) === expected && fs.lstatSync(file).isFile() && fs.statSync(file).size <= MAX_BYTES;
  } catch { return false; }
}

/** Same native-history gate for an unpublished pending Edit, independent of its display. */
export function hasPendingAutoplanArtifactHistory(
  context: ArtifactPermissionContext & { pending?: PendingAutoplanArtifact },
): boolean {
  const p = context.pending, now = context.now ?? Date.now();
  if (!p || p.source !== 'pre_tool_use' || p.tool !== 'Edit' || context.transcriptStatus !== 'ready' ||
      !Number.isFinite(now) || !Number.isFinite(context.commandStartedAt) ||
      !/^[A-Za-z0-9_-]{1,160}$/.test(p.sessionId) || !/^[A-Za-z0-9_-]{1,160}$/.test(p.toolUseId) ||
      !ownedAutoplanArtifact(p.file, context) || context.publicTools.length > 10_000) return false;
  const pendingTime = Date.parse(p.timestamp);
  if (!Number.isFinite(pendingTime) || pendingTime < context.commandStartedAt || pendingTime > now) return false;
  const events = context.publicTools.filter(e => Date.parse(e.timestamp) >= context.commandStartedAt);
  if (!events.length || context.publicTools.some(e => !Number.isFinite(Date.parse(e.timestamp)))) return false;
  const uses = new Map<string, NativePublicToolEvent>(), results = new Map<string, NativePublicToolEvent>();
  let last = context.commandStartedAt;
  for (const event of events) {
    const time = Date.parse(event.timestamp);
    if (event.sessionId !== p.sessionId || !event.toolUseId || event.toolUseId === p.toolUseId || time < last || time > now) return false;
    last = time;
    const map = event.kind === 'use' ? uses : results;
    if (map.has(event.toolUseId) || (event.kind === 'result' && !uses.has(event.toolUseId))) return false;
    map.set(event.toolUseId, event);
  }
  const mutations = [...uses.values()].filter(e => e.name === 'Write' || e.name === 'Edit');
  // Hook metadata cannot replace a published request/result or an unresolved
  // mutation. Public successful same-file history remains mandatory.
  if (mutations.some(e => !results.has(e.toolUseId) || Date.parse(e.timestamp) > pendingTime ||
      Date.parse(results.get(e.toolUseId)!.timestamp) > pendingTime) ||
      !mutations.some(e => e.input?.file_path === p.file && results.get(e.toolUseId)?.isError === false &&
        Date.parse(results.get(e.toolUseId)!.timestamp) <= pendingTime)) return false;
  return true;
}
