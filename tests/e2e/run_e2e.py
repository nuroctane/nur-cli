"""Black-box E2E suite: drive the real `nur` binary against a scripted provider.

Every scenario runs `nur run` in a throwaway home and workspace, with a fake
OpenAI-compatible server (fake_provider.py) standing in for the model. Checks
are on what a user can observe - files on disk, stdout, exit code - and on
what nur actually sent the model (tool results fed back, request count).

Each run leaves a repeatable artifact under target/e2e/: per-scenario stdout,
stderr, the full request log and a report, plus target/e2e/report.md.

Usage:
  python tests/e2e/run_e2e.py [--bin PATH] [scenario-name ...]
The binary defaults to target/release/nur(.exe), then target/debug, then PATH.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OUT = ROOT / "target" / "e2e"
EXE = ".exe" if os.name == "nt" else ""
TIMEOUT_SECS = 180
WARM_CACHE = OUT / ".warm-cache"


def find_binary(explicit):
    if explicit:
        return Path(explicit)
    for profile in ("release", "debug"):
        candidate = ROOT / "target" / profile / f"nur{EXE}"
        if candidate.exists():
            return candidate
    found = shutil.which("nur")
    if not found:
        sys.exit("no nur binary: build it or pass --bin")
    return Path(found)


def start_provider(script, workdir):
    script_file = workdir / "script.json"
    script_file.write_text(json.dumps(script), encoding="utf-8")
    log = workdir / "requests.jsonl"
    log.write_text("", encoding="utf-8")
    port_file = workdir / "port"
    proc = subprocess.Popen(
        [sys.executable, str(HERE / "fake_provider.py"), str(script_file), str(log), str(port_file)]
    )
    deadline = time.time() + 10
    while not (port_file.exists() and port_file.read_text().strip()):
        if time.time() > deadline or proc.poll() is not None:
            stop_tree(proc)
            raise RuntimeError("fake provider did not start")
        time.sleep(0.05)
    return proc, int(port_file.read_text()), log


def stop_tree(proc):
    """Kill a process and its children. On Windows sys.executable can be a venv
    launcher whose real interpreter is a child that plain kill() leaves running."""
    if os.name == "nt":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True)
    else:
        proc.kill()
    proc.wait()


def isolated_env(home, port):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("NUR_", "OPENAI_", "ANTHROPIC_"))}
    env.update(
        {
            "NUR_HOME": str(home / ".nur"),
            # Unix dirs::home_dir() reads HOME. On Windows it asks the OS for the
            # profile folder and ignores both, so there the real ~ stays visible
            # (vendor CLI sessions, ~/.agents/skills, ~/.optmem): scenarios must
            # not depend on what is or is not in it.
            "HOME": str(home),
            "USERPROFILE": str(home),
            "NUR_SKIP_BOOTSTRAP": "1",
            "NUR_SKIP_AUTO_UPDATE": "1",
            "NUR_DISABLE_UPDATES": "1",
            "NUR_PRICING_OFF": "1",
            "NUR_MODELS_DEV_OFF": "1",
            "NO_COLOR": "1",
        }
    )
    nur_home = home / ".nur"
    nur_home.mkdir(parents=True, exist_ok=True)
    # The skill index scans every global skill root cold (thousands of files
    # on a developer machine). Reuse the first scenario's index so the suite
    # measures nur, not one cold scan per scenario.
    if WARM_CACHE.exists():
        shutil.copytree(WARM_CACHE, nur_home / "cache", dirs_exist_ok=True)
    (nur_home / "config.toml").write_text(
        f'provider = "vllm"\nbase_url = "http://127.0.0.1:{port}/v1"\nmodel = "e2e-model"\n',
        encoding="utf-8",
    )
    return env


def tool_messages(request):
    return [
        str(m.get("content", ""))
        for m in request.get("messages", [])
        if m.get("role") == "tool"
    ]


def check(scenario, result, workspace, requests):
    expect = scenario.get("expect", {})
    failures = []
    if result.returncode != expect.get("exit_code", 0):
        failures.append(f"exit code {result.returncode}, expected {expect.get('exit_code', 0)}")
    for needle in expect.get("stdout_contains", []):
        if needle not in result.stdout:
            failures.append(f"stdout lacks {needle!r}")
    for needle in expect.get("stdout_lacks", []):
        if needle in result.stdout:
            failures.append(f"stdout contains {needle!r}")
    for needle in expect.get("stderr_contains", []):
        if needle not in result.stderr:
            failures.append(f"stderr lacks {needle!r}")
    sent = "\n".join(json.dumps(r) for r in requests)
    for needle in expect.get("requests_lack", []):
        if needle in sent:
            failures.append(f"a model request contains {needle!r}")
    if "max_tool_result_chars" in expect:
        longest = max((len(t) for r in requests for t in tool_messages(r)), default=0)
        if longest > expect["max_tool_result_chars"]:
            failures.append(f"a tool result of {longest} chars reached the model")
    for rel, want in expect.get("files", {}).items():
        path = workspace / rel
        if not path.exists():
            failures.append(f"missing file {rel}")
            continue
        text = path.read_text(encoding="utf-8")
        if isinstance(want, dict):
            for needle in want.get("contains", []):
                if needle not in text:
                    failures.append(f"{rel} lacks {needle!r}")
            for needle in want.get("lacks", []):
                if needle in text:
                    failures.append(f"{rel} still contains {needle!r}")
        elif text != want:
            failures.append(f"{rel} is {text!r}, expected {want!r}")
    for rel in expect.get("absent_files", []):
        if (workspace / rel).exists():
            failures.append(f"{rel} exists but must not")
    if "requests" in expect and len(requests) != expect["requests"]:
        failures.append(f"{len(requests)} model requests, expected {expect['requests']}")
    for rule in expect.get("tool_results", []):
        n = rule["request"]
        if n > len(requests):
            failures.append(f"request {n} never happened")
            continue
        results = tool_messages(requests[n - 1])
        for needle in rule.get("contains", []):
            if not any(needle in r for r in results):
                failures.append(f"request {n} tool results lack {needle!r}: {results!r}"[:400])
    return failures


def remove_tree(path):
    """Delete a scenario's temp tree; return a locked path if something holds one."""
    for _ in range(20):
        try:
            shutil.rmtree(path)
            return None
        except FileNotFoundError:
            return None
        except PermissionError as e:
            locked = e.filename
            time.sleep(0.25)
    shutil.rmtree(path, ignore_errors=True)
    return locked


def run_scenario(binary, scenario_path):
    scenario = json.loads(scenario_path.read_text(encoding="utf-8"))
    name = scenario_path.stem
    if scenario.get("skip"):
        # An open product decision, recorded as a scenario so it stays visible.
        return {"scenario": name, "skipped": scenario["skip"], "passed": True, "failures": [],
                "model_requests": 0, "seconds": 0}
    out = OUT / name
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    tmp = Path(tempfile.mkdtemp(prefix=f"nur-e2e-{name}-"))
    try:
        workspace = tmp / "workspace"
        workspace.mkdir()
        for rel, text in scenario.get("setup_files", {}).items():
            (workspace / rel).parent.mkdir(parents=True, exist_ok=True)
            (workspace / rel).write_text(text, encoding="utf-8")
        provider, port, log = start_provider(scenario["script"], tmp)
        env = isolated_env(tmp / "home", port)
        # Files under the scenario's nur home (hooks.toml, config additions).
        # "{home}" and "{workspace}" are replaced with the real paths.
        nur_home = Path(env["NUR_HOME"])
        for rel, text in scenario.get("setup_home_files", {}).items():
            text = text.replace("{home}", nur_home.as_posix()).replace("{workspace}", workspace.as_posix())
            (nur_home / rel).parent.mkdir(parents=True, exist_ok=True)
            (nur_home / rel).write_text(text, encoding="utf-8")
        try:
            args = [str(binary), "--cwd", str(workspace), "run"]
            mode = scenario.get("mode", "auto")
            args += ["--yes"] if mode == "auto" else ["--mode", mode]
            args.append(scenario["prompt"])
            started = time.time()
            child = subprocess.Popen(
                args,
                env=env,
                cwd=workspace,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                stdin=subprocess.DEVNULL,
                text=True,
                encoding="utf-8",
                errors="replace",
            )
            try:
                stdout, stderr = child.communicate(timeout=TIMEOUT_SECS)
                result = subprocess.CompletedProcess(args, child.returncode, stdout, stderr)
            except subprocess.TimeoutExpired:
                # The whole tree: a timed-out nur can have children running.
                stop_tree(child)
                stdout, stderr = child.communicate()
                result = subprocess.CompletedProcess(args, -1, stdout or "", (stderr or "") + "\n[timed out]")
            elapsed = time.time() - started
        finally:
            stop_tree(provider)
        requests = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines() if line]
        index = tmp / "home" / ".nur" / "cache" / "skills-index.json"
        if index.exists() and not WARM_CACHE.exists():
            WARM_CACHE.mkdir(parents=True)
            shutil.copy(index, WARM_CACHE / index.name)
        failures = check(scenario, result, workspace, requests)
        files = sorted(str(p.relative_to(workspace)) for p in workspace.rglob("*") if p.is_file())
        shutil.copy(log, out / "requests.jsonl")
    finally:
        leftover = remove_tree(tmp)
    if leftover:
        # A one-shot run must not leave a process behind holding its home.
        failures.append(f"a process spawned by the run still holds {leftover}")
    (out / "stdout.txt").write_text(result.stdout, encoding="utf-8")
    (out / "stderr.txt").write_text(result.stderr, encoding="utf-8")
    report = {
        "scenario": name,
        "description": scenario.get("description", ""),
        "passed": not failures,
        "failures": failures,
        "exit_code": result.returncode,
        "model_requests": len(requests),
        "workspace_files": files,
        "seconds": round(elapsed, 2),
    }
    (out / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin")
    parser.add_argument("names", nargs="*")
    opts = parser.parse_args()
    binary = find_binary(opts.bin)
    paths = sorted((HERE / "scenarios").glob("*.json"))
    if opts.names:
        paths = [p for p in paths if p.stem in opts.names]
    OUT.mkdir(parents=True, exist_ok=True)
    # One fresh index per suite run: a copy older than nur's 24h TTL would be
    # ignored, silently putting every scenario back on the cold scan.
    shutil.rmtree(WARM_CACHE, ignore_errors=True)
    reports = [run_scenario(binary, p) for p in paths]
    lines = [
        f"# nur E2E report\n\nbinary: `{binary}`\n",
        "| scenario | result | requests | seconds | notes |",
        "|---|---|---|---|---|",
    ]
    for r in reports:
        notes = "; ".join(r["failures"]).replace("|", "\\|")
        result = "skip" if r.get("skipped") else ("pass" if r["passed"] else "FAIL")
        notes = r.get("skipped") or notes
        lines.append(f"| {r['scenario']} | {result} | {r['model_requests']} | {r['seconds']} | {notes} |")
    (OUT / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    for r in reports:
        if r.get("skipped"):
            print(f"SKIP  {r['scenario']}  ({r['skipped']})")
            continue
        print(f"{'PASS' if r['passed'] else 'FAIL'}  {r['scenario']}")
        for f in r["failures"]:
            print(f"      {f}")
    failed = sum(not r["passed"] for r in reports)
    print(f"\n{len(reports) - failed}/{len(reports)} passed · artifacts in {OUT}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
