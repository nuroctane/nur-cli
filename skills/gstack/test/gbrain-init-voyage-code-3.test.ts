/**
 * setup-gbrain's local PGLite init sequences, executed from the TEMPLATE.
 *
 * The contract lives in skill template prose the model executes, not in a TS
 * helper, so this file extracts each fenced bash block that runs
 * `gbrain init --pglite --json "$@"` from the .tmpl files and runs it against
 * a fake `gbrain` in a sandboxed HOME. A template edit changes what runs here;
 * there is no hand-copied shell to drift.
 *
 * Contracts:
 *   1. voyage-code-3 default: with VOYAGE_API_KEY set, every init site passes
 *      --embedding-model voyage:voyage-code-3 --embedding-dimensions 1024 as
 *      separate argv words (also under zsh, #1798); unset or empty omits them.
 *   2. .bak rollback (plan D7): the two rollback-wrapped sites move an existing
 *      ~/.gbrain/config.json aside, restore it byte-for-byte when init fails
 *      (leaving a partial PGLite dir alone), and keep the backup for audit when
 *      init succeeds.
 */

import { describe, it, expect } from "bun:test";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  readdirSync,
  existsSync,
  rmSync,
  chmodSync,
} from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { spawnSync } from "child_process";

const SETUP_GBRAIN = join(import.meta.dir, "..", "setup-gbrain");
const TEMPLATES = {
  skeleton: join(SETUP_GBRAIN, "SKILL.md.tmpl"),
  brainInit: join(SETUP_GBRAIN, "sections", "brain-init.md.tmpl"),
  remediation: join(SETUP_GBRAIN, "sections", "engine-remediation.md.tmpl"),
};
const INIT_CALL = 'gbrain init --pglite --json "$@"';

function initBlocks(tmplPath: string): string[] {
  const src = readFileSync(tmplPath, "utf-8");
  return [...src.matchAll(/```bash\n([\s\S]*?)```/g)]
    .map((m) => m[1])
    .filter((block) => block.includes(INIT_CALL));
}

const PATH3_BLOCK = initBlocks(TEMPLATES.brainInit).find((b) => !b.includes("gstack-bak"));
const PATH4_BLOCK = initBlocks(TEMPLATES.brainInit).find((b) => b.includes("gstack-bak"));
const REMEDIATION_BLOCK = initBlocks(TEMPLATES.remediation).find((b) => b.includes("gstack-bak"));
const ROLLBACK_SITES = { "Path 4 local code search": PATH4_BLOCK, "engine remediation": REMEDIATION_BLOCK };
const ALL_SITES = { "Path 3 PGLite": PATH3_BLOCK, ...ROLLBACK_SITES };

interface Sandbox {
  home: string;
  bindir: string;
  configPath: string;
  argvLog: string;
  cleanup: () => void;
}

function makeSandbox(opts: { initFails?: boolean; seedConfig?: boolean } = {}): Sandbox {
  const tmp = mkdtempSync(join(tmpdir(), "gbrain-pglite-init-"));
  const home = join(tmp, "home");
  const gbrainDir = join(home, ".gbrain");
  const bindir = join(tmp, "bin");
  const configPath = join(gbrainDir, "config.json");
  const argvLog = join(tmp, "gbrain-argv.log");
  mkdirSync(gbrainDir, { recursive: true });
  mkdirSync(bindir, { recursive: true });
  const installer = join(home, ".claude", "skills", "gstack", "bin", "gstack-gbrain-install");
  mkdirSync(join(installer, ".."), { recursive: true });
  writeFileSync(installer, "#!/bin/sh\nexit 0\n");
  chmodSync(installer, 0o755);
  if (opts.seedConfig) {
    writeFileSync(configPath, JSON.stringify({ engine: "postgres", database_url: "postgresql://stale@localhost/gbrain" }));
  }
  const onInit = opts.initFails
    ? `mkdir -p "${gbrainDir}/pglite" && : > "${gbrainDir}/pglite/partial-write.tmp"; echo "Error: disk full" >&2; exit 1`
    : `printf '{"engine":"pglite"}' > "${configPath}"; echo '{"status":"success"}'; exit 0`;
  writeFileSync(
    join(bindir, "gbrain"),
    `#!/bin/sh\necho "$@" >> "${argvLog}"\necho "$#" >> "${argvLog}.argc"\nif [ "$1" = "init" ]; then ${onInit}; fi\nexit 0\n`,
  );
  chmodSync(join(bindir, "gbrain"), 0o755);
  return { home, bindir, configPath, argvLog, cleanup: () => rmSync(tmp, { recursive: true, force: true }) };
}

function runBlock(sb: Sandbox, block: string, opts: { voyageKey?: string; shell?: "bash" | "zsh" } = {}) {
  const env: Record<string, string> = { ...process.env, HOME: sb.home, PATH: `${sb.bindir}:/usr/bin:/bin` };
  delete env.VOYAGE_API_KEY;
  if (opts.voyageKey !== undefined) env.VOYAGE_API_KEY = opts.voyageKey;
  const r = spawnSync(opts.shell ?? "bash", ["-c", block], { encoding: "utf-8", env, timeout: 30_000 });
  const argv = existsSync(sb.argvLog) ? readFileSync(sb.argvLog, "utf-8").trim().split("\n") : [];
  const argc = existsSync(`${sb.argvLog}.argc`)
    ? readFileSync(`${sb.argvLog}.argc`, "utf-8").trim().split("\n").map(Number)
    : [];
  return { status: r.status, stderr: r.stderr ?? "", argv, argc };
}

function backups(sb: Sandbox): string[] {
  return readdirSync(join(sb.home, ".gbrain")).filter((f) => f.includes(".gstack-bak-"));
}

const HAVE_ZSH = spawnSync("zsh", ["-c", "true"], { timeout: 30_000 }).status === 0;

describe("template extraction", () => {
  it("finds all three PGLite init blocks (a template restructure must update this file)", () => {
    expect(PATH3_BLOCK).toBeDefined();
    expect(PATH4_BLOCK).toBeDefined();
    expect(REMEDIATION_BLOCK).toBeDefined();
    expect(initBlocks(TEMPLATES.skeleton)).toEqual([]);
  });
});

describe("voyage-code-3 default at every PGLite init site", () => {
  for (const [site, block] of Object.entries(ALL_SITES)) {
    it(`${site}: passes voyage-code-3 flags when VOYAGE_API_KEY is set`, () => {
      const sb = makeSandbox();
      try {
        const r = runBlock(sb, block!, { voyageKey: "vk_test_set" });
        expect(r.argv).toEqual(["init --pglite --json --embedding-model voyage:voyage-code-3 --embedding-dimensions 1024"]);
        expect(r.argc).toEqual([7]);
      } finally {
        sb.cleanup();
      }
    });

    it(`${site}: omits voyage flags when VOYAGE_API_KEY is unset or empty`, () => {
      for (const voyageKey of [undefined, ""]) {
        const sb = makeSandbox();
        try {
          const r = runBlock(sb, block!, { voyageKey });
          expect(r.argv).toEqual(["init --pglite --json"]);
        } finally {
          sb.cleanup();
        }
      }
    });

    it(`${site}: zsh passes the flags as SEPARATE argv words (#1798)`, () => {
      if (!HAVE_ZSH) return;
      const sb = makeSandbox();
      try {
        expect(runBlock(sb, block!, { voyageKey: "vk_test_set", shell: "zsh" }).argc).toEqual([7]);
      } finally {
        sb.cleanup();
      }
    });
  }
});

describe(".bak rollback contract (plan D7)", () => {
  for (const [site, block] of Object.entries(ROLLBACK_SITES)) {
    it(`${site}: failed init restores the original config and leaves partial PGLite state alone`, () => {
      const sb = makeSandbox({ initFails: true, seedConfig: true });
      try {
        const original = readFileSync(sb.configPath, "utf-8");
        const r = runBlock(sb, block!);
        expect(r.stderr).toContain("restored");
        expect(readFileSync(sb.configPath, "utf-8")).toBe(original);
        expect(backups(sb)).toEqual([]);
        expect(existsSync(join(sb.home, ".gbrain", "pglite", "partial-write.tmp"))).toBe(true);
      } finally {
        sb.cleanup();
      }
    });

    it(`${site}: successful init installs the new config and keeps the backup for audit`, () => {
      const sb = makeSandbox({ seedConfig: true });
      try {
        const r = runBlock(sb, block!);
        expect(r.status).toBe(0);
        expect(JSON.parse(readFileSync(sb.configPath, "utf-8")).engine).toBe("pglite");
        expect(backups(sb).length).toBe(1);
      } finally {
        sb.cleanup();
      }
    });
  }

  it("Path 4 continues setup after a failed init; engine remediation stops with exit 1", () => {
    const path4 = makeSandbox({ initFails: true, seedConfig: true });
    const remediation = makeSandbox({ initFails: true, seedConfig: true });
    try {
      const p4 = runBlock(path4, PATH4_BLOCK!);
      expect(p4.status).toBe(0);
      expect(p4.stderr).toContain("Continuing setup without local code search");
      expect(runBlock(remediation, REMEDIATION_BLOCK!).status).toBe(1);
    } finally {
      path4.cleanup();
      remediation.cleanup();
    }
  });

  it("Path 4 with no existing config: failed init creates no backup and no config", () => {
    const sb = makeSandbox({ initFails: true });
    try {
      runBlock(sb, PATH4_BLOCK!);
      expect(backups(sb)).toEqual([]);
      expect(existsSync(sb.configPath)).toBe(false);
    } finally {
      sb.cleanup();
    }
  });
});

describe("template alignment", () => {
  const tmpl = Object.values(TEMPLATES).map((p) => readFileSync(p, "utf-8")).join("\n");

  it("uses the positional-params shape at all 3 init sites, never an unquoted flags var", () => {
    expect(tmpl).not.toContain("$GBRAIN_EMBED_FLAGS");
    expect(tmpl.match(/gbrain init --pglite --json "\$@"/g)?.length).toBe(3);
    expect(tmpl.match(/set -- --embedding-model voyage:voyage-code-3 --embedding-dimensions 1024/g)?.length).toBe(3);
    expect(tmpl.match(/if \[ -n "\$\{VOYAGE_API_KEY:-\}" \]; then/g)?.length).toBe(3);
  });
});
