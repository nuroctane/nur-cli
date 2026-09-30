/**
 * Periodic-lane exclusions — LITERALS ONLY (own file, deliberately NOT in
 * touchfiles-data.ts: that file is evaluated standalone by map-diff against
 * old git versions, and its contract must not grow unrelated exports).
 *
 * The weekly periodic CI lane runs EVERY periodic-tier file (EVALS_ALL=1) so
 * tests can't rot invisibly — the coverage contract. A file lands here only
 * when running it weekly is KNOWN waste (documented-red or requires manual
 * hardware), and every entry must carry a tracking pointer with a re-entry
 * condition, so an exclusion is a decision with an owner, not a place tests
 * go to die. Pinned by test/periodic-exclude-policy.test.ts: entries must
 * name real files and carry non-empty reason + tracking.
 *
 * Removing an entry re-activates the file on the next weekly run — that IS
 * the re-entry mechanism.
 */
export const PERIODIC_CI_EXCLUDE: Record<string, { reason: string; tracking: string }> = {
  'test/codex-e2e.test.ts': {
    reason: 'the codex CLI is not installed in the CI image (Dockerfile.ci ships only claude-code); every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/codex-e2e-sol-scope.test.ts': {
    reason: 'the codex CLI is not installed in the CI image (Dockerfile.ci ships only claude-code); every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/codex-e2e-shared-libs.test.ts': {
    reason: 'the codex CLI is not installed in the CI image (Dockerfile.ci ships only claude-code); every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/codex-e2e-recommendation-substance.test.ts': {
    reason: 'the codex CLI is not installed in the CI image (Dockerfile.ci ships only claude-code); every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/skill-e2e-outside-voice.test.ts': {
    reason: 'needs both the claude and codex CLIs; codex is not in the CI image, so every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/skill-e2e-aside.test.ts': {
    reason: 'needs macOS with the Aside app open (asideAvailable()); CI runners are Linux, so every case self-skips',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
  'test/skill-e2e-ios-device.test.ts': {
    reason: 'needs a physical iPhone over USB/devicectl — manual hardware, not a CI runner capability',
    tracking: 'TODOS.md "CI-unrunnable paid evals" (re-entry: the CLI/device is available in the CI image; review by 2026-12-28)',
  },
};
