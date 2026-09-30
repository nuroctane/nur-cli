<!-- AUTO-GENERATED from shared-code-reuse.md.tmpl — do not edit directly -->
<!-- Regenerate: bun run gen:skill-docs -->
**Reuse a skipped shared-code advisory only with complete structural evidence:**

1. **Read the evidence.** Read all supporting callers and the helper destination.
   Establish first-party authored provenance and whether the current extraction
   is worthwhile; the checker cannot decide that. Retain `evidence_paths`/`helper_target`.
2. **Run the checker.** From the repository root, pass the current finding as
   literal JSON on stdin. Replace REVIEW_START with this pass's captured token
   and the example paths/symbol with actual evidence. Keep the quoted delimiter.

```bash
"$HOME/.claude/skills/gstack/bin/gstack-review-log" --check-shared-libs REVIEW_START <<'GSTACK_SHARED_LIBS_REUSE_JSON'
{"advisory":true,"severity":"INFORMATIONAL","evidence_paths":["src/caller-a.ts","src/caller-b.ts"],"helper_target":{"path":"src/shared.ts","symbol":"sharedHelper"}}
GSTACK_SHARED_LIBS_REUSE_JSON
```

3. **Act on its result.** Read the JSON. Only `reusable: true` permits suppression.
   False, command failure or unreadable output requires fresh source review and a
   new decision, never suppression. Do not supply your own snapshot, prior record or coverage.
4. **Persist through the logger.** The logger recomputes final coverage; never
   supply proof yourself. Real defects retain normal Fix-First handling independently.

**What a reusable result proves (do not reconstruct these checks yourself):**
- Identity: `sharedLibsFingerprint` plus the actual repo, raw branch and current snapshot.
  The checker reads REVIEW_START without consuming/replacing it. Sanitized branch names are not identity.
- Prior decision: completed/converged review, verified binding, explicit Skip and
  logger-versioned `snapshot_covered_paths`; older unversioned coverage needs a fresh decision.
- Source: `canReuseSharedLibsAdvisory` requires every supporting path's raw file
  byte-for-byte with its blob. Exclude assume-unchanged, skip-worktree and sparse index
  entries; symlinks/ancestors, submodules, ignored/outside or unreadable files;
  active/unknown Git filters, encodings and line conversion.
- Safe inspection: disables fsmonitor and optional locks; never uses external diff/textconv.
  Unknown evidence fails closed.
