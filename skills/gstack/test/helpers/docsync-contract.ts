import * as path from 'node:path';

export interface DocsCompletion {
  schema_version: 1;
  audit_id: string;
  status: 'updated' | 'current' | 'blocked';
  files_updated: string[];
  files_reviewed: string[];
  documentation_section: string;
  blockers: string[];
  decisions: string[];
}

const keys = ['schema_version', 'audit_id', 'status', 'files_updated', 'files_reviewed', 'documentation_section', 'blockers', 'decisions'];

export function parseDocsCompletion(output: string, auditId: string): DocsCompletion {
  const result = JSON.parse(output.trimEnd().split('\n').at(-1)!);
  if (!result || Array.isArray(result) || typeof result !== 'object' ||
      Object.keys(result).sort().join() !== [...keys].sort().join()) throw new Error('completion fields');
  if (result.schema_version !== 1 || result.audit_id !== auditId) throw new Error('completion identity');
  if (!['updated', 'current', 'blocked'].includes(result.status)) throw new Error('completion status');
  for (const field of ['files_updated', 'files_reviewed', 'blockers', 'decisions']) {
    if (!Array.isArray(result[field]) || result[field].some((v: unknown) => typeof v !== 'string' || !v.trim())) {
      throw new Error(`completion ${field}`);
    }
  }
  if (typeof result.documentation_section !== 'string' || !result.documentation_section.trim()) throw new Error('completion markdown');
  for (const field of ['files_updated', 'files_reviewed']) {
    const paths: string[] = result[field];
    if (new Set(paths).size !== paths.length || paths.some(p => path.posix.isAbsolute(p) || p.includes('\\') ||
      p.split('/').some(part => !part || part === '.' || part === '..') || /[*?\[\]]/.test(p))) throw new Error('completion paths');
  }
  if (result.status === 'blocked' ? result.blockers.length === 0 : result.blockers.length !== 0) throw new Error('completion blockers');
  if (result.status === 'current' && result.files_updated.length !== 0 ||
      result.status === 'updated' && result.files_updated.length === 0) throw new Error('completion edits');
  return result;
}

export function vetDocsCompletion(result: DocsCompletion, evidence: {
  settled: boolean;
  markerSeen: boolean;
  headUnchanged: boolean;
  indexUnchanged: boolean;
  candidateUnchanged: boolean;
  readOnly: boolean;
  changedPaths: string[];
  allowedDocs: string[];
}): void {
  if (!evidence.settled || !evidence.markerSeen) throw new Error('unsettled or unmarked child');
  if (!evidence.headUnchanged || !evidence.indexUnchanged) throw new Error('Git ownership violation');
  if (!evidence.candidateUnchanged) throw new Error('stale candidate');
  if (evidence.readOnly && evidence.changedPaths.length) throw new Error('read-only mutation');
  if ([...evidence.changedPaths].sort().join('\0') !== [...result.files_updated].sort().join('\0')) throw new Error('unreported edits');
  const forbidden = /(?:^|\/)(?:VERSION|CHANGELOG(?:\.[^/]*)?|TODOS(?:\.[^/]*)?|package(?:-lock)?\.json|[^/]*lock[^/]*|manifest\.json)$/i;
  if (evidence.changedPaths.some(p => forbidden.test(p) || !evidence.allowedDocs.includes(p))) throw new Error('non-doc mutation');
}

export function extractDocsDispatch(section: string): string {
  const begin = section.indexOf('**Subagent prompt:**');
  const end = section.indexOf('**Parent processing:**');
  if (begin < 0 || end <= begin) throw new Error('documentation dispatch markers moved');
  return section.slice(begin + '**Subagent prompt:**'.length, end)
    .split('\n').map(line => line.replace(/^> ?/, '')).join('\n').trim();
}

export function docsDispatchIndex(calls: Array<{ tool: string; input: unknown }>): number {
  return calls.findIndex(call => ['Agent', 'Task'].includes(call.tool) &&
    /document-release\/SKILL\.md|executing the \/document-release workflow/i.test(JSON.stringify(call.input)) &&
    !/Parent processing:|## Step 19: Create PR\/MR/.test(JSON.stringify(call.input)));
}
