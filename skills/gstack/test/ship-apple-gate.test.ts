/**
 * R2 pin (fork port wave 2): the Apple release adapter loads BEFORE ship's
 * branch gate, and the non-Apple gate is byte-unchanged.
 *
 * Two failure modes this prevents: a future ship-template refactor that
 * re-blocks store releases behind "ship from a feature branch" (the exact
 * live failure the fork hit — a solo dev with a clean tree on main shipping
 * to TestFlight got aborted over branch topology), and the reverse — the
 * Apple path accidentally weakening the branch gate for normal
 * repository-landing ships.
 */

import { describe, test, expect } from "bun:test";
import { readFileSync } from "fs";
import { join } from "path";

const ROOT = join(import.meta.dir, "..");
const SKELETON = readFileSync(join(ROOT, "ship", "SKILL.md"), "utf-8");

const GATE_TEXT =
  'If on the base branch or the repo\'s default branch, **abort**: "You\'re on the base branch. Ship from a feature branch."';

describe("ship Apple gate ordering (R2)", () => {
  test("the section index requires a store-distribution request, not merely an Apple repository", () => {
    const manifest = JSON.parse(readFileSync(join(ROOT, "ship", "sections", "manifest.json"), "utf-8"));
    const apple = manifest.sections.find((section: { id: string }) => section.id === "apple-release");
    expect(apple.trigger).toContain("App Store/TestFlight distribution is requested for an Apple app");
    expect(apple.trigger).toContain("an Apple repository-landing request follows the normal pipeline");
    expect(SKELETON).toContain("is App Store/TestFlight distribution");
  });

  test("the Apple adapter read directive precedes the branch gate", () => {
    const appleRead = SKELETON.indexOf("sections/apple-release.md");
    const gate = SKELETON.indexOf(GATE_TEXT);
    expect(appleRead).toBeGreaterThan(-1);
    expect(gate).toBeGreaterThan(-1);
    expect(appleRead).toBeLessThan(gate);
  });

  test("store distribution explicitly bypasses the branch/PR ceremony", () => {
    expect(SKELETON).toContain("Store distribution proceeds");
    expect(SKELETON).toMatch(/branch gate and repository-landing pipeline below apply ONLY to\s*\n?repository-landing asks/);
  });

  test("the non-Apple branch gate is byte-unchanged and appears exactly once", () => {
    const first = SKELETON.indexOf(GATE_TEXT);
    expect(first).toBeGreaterThan(-1);
    expect(SKELETON.indexOf(GATE_TEXT, first + 1)).toBe(-1);
  });

  test("the adapter section exists in the union with its battle-tested spine", () => {
    const section = readFileSync(join(ROOT, "ship", "sections", "apple-release.md"), "utf-8");
    for (const anchor of [
      "one authorization moment",
      "fastlane spaceauth",
      "iris/v1/apiKeys",
      "appPriceSchedules",
      "CLASSIFY the error before touching credentials",
      "Never abort an App Store release over branch topology",
    ]) {
      expect(section).toContain(anchor);
    }
  });

  test("routine interaction limits cannot waive blocking documentation or safety decisions", () => {
    for (const name of ["apple-release.md.tmpl", "apple-release.md"]) {
      const section = readFileSync(join(ROOT, "ship", "sections", name), "utf-8");
      expect(section).toContain("Plan for two routine interactions");
      expect(section).toContain("A genuine blocker may require a safety or named documentation-risk decision");
      expect(section).toContain("STOP for that decision rather than treating release authorization as a waiver");
      expect(section).toContain("routine interactions and blocking decisions above");
      expect(section).not.toContain("exactly two interactions, and no others");
      expect(section).not.toContain("two permitted interactions");
      expect(section.indexOf("**Documentation preflight:**")).toBeLessThan(section.indexOf("## The one authorization moment"));
      expect(section).toContain("in `read-only` mode against the selected release source");
      expect(section).toContain("Resolve blockers or obtain an explicit named documentation-risk exception before distribution");
    }
  });
});
