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
        # Scenarios change cwd to a throwaway workspace. Resolve while still
        # in the caller's directory; Unix exec resolves relative paths after cwd.
        return Path(explicit).expanduser().resolve()
    for profile in ("release", "debug"):
        candidate = ROOT / "target" / profile / f"nur{EXE}"
        if candidate.exists():
            return candidate
    found = shutil.which("nur")
    if not found:
        sys.exit("no nur binary: build it or pass --bin")
    return Path(found)


def spawn_process(*args, **kwargs):
    proc = subprocess.Popen(*args, **kwargs)
    if os.name == "nt":
        # The uv/venv launcher may own a second interpreter. Track the owned
        # tree natively so test cleanup cannot stall in taskkill's WMI lookup.
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        kernel.CreateJobObjectW.restype = wintypes.HANDLE
        kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        kernel.AssignProcessToJobObject.restype = wintypes.BOOL
        kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        kernel.TerminateJobObject.restype = wintypes.BOOL
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel.CloseHandle.restype = wintypes.BOOL
        job = kernel.CreateJobObjectW(None, None)
        if job and kernel.AssignProcessToJobObject(job, int(proc._handle)):
            proc._nur_job = (kernel, job)
        elif job:
            kernel.CloseHandle(job)
    return proc


def start_provider(script, workdir, mcp=None):
    script_file = workdir / "script.json"
    script_file.write_text(json.dumps(script), encoding="utf-8")
    log = workdir / "requests.jsonl"
    log.write_text("", encoding="utf-8")
    port_file = workdir / "port"
    argv = [sys.executable, str(HERE / "fake_provider.py"), str(script_file), str(log), str(port_file)]
    if mcp is not None:
        # The same server also plays a remote MCP server (see FakeMcp).
        (workdir / "mcp.json").write_text(json.dumps(mcp), encoding="utf-8")
        (workdir / "mcp.jsonl").write_text("", encoding="utf-8")
        argv += [str(workdir / "mcp.json"), str(workdir / "mcp.jsonl")]
    proc = spawn_process(argv)
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
    if proc.poll() is not None and not getattr(proc, "_nur_job", None):
        return
    if os.name == "nt":
        tracking = getattr(proc, "_nur_job", None)
        if tracking:
            kernel, job = tracking
            kernel.TerminateJobObject(job, 1)
            kernel.CloseHandle(job)
            del proc._nur_job
        else:
            try:
                subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True, timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
    else:
        proc.kill()
    if proc.poll() is None:
        proc.kill()
    proc.wait(timeout=5)


def isolated_env(home, port):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("NUR_", "OPENAI_", "ANTHROPIC_", "ENCLAVE_"))}
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
    out = [str(m.get("content", "")) for m in request.get("messages", []) if m.get("role") == "tool"]
    out += [str(item.get("output", "")) for item in request.get("input", []) if isinstance(item, dict) and item.get("type") == "function_call_output"]
    for message in request.get("messages", []):
        content = message.get("content", [])
        if isinstance(content, list):
            out += [str(item.get("content", "")) for item in content if item.get("type") == "tool_result"]
    return out


def check_mcp(expect, mcp_requests):
    """What nur sent the remote MCP server: bearer, method order, call arguments."""
    failures = []
    bearer = expect.get("bearer")
    if bearer and any(r.get("auth") != f"Bearer {bearer}" for r in mcp_requests):
        failures.append("an MCP request lacked the expected bearer key")
    methods = [r.get("method") for r in mcp_requests]
    if "methods" in expect and methods != expect["methods"]:
        failures.append(f"MCP methods {methods}, expected {expect['methods']}")
    for tool, arguments in expect.get("call_arguments", {}).items():
        calls = [(r.get("params") or {}) for r in mcp_requests if r.get("method") == "tools/call"]
        if not any(c.get("name") == tool and c.get("arguments") == arguments for c in calls):
            failures.append(f"no tools/call of {tool} with {arguments!r}")
    return failures


def check(scenario, result, workspace, requests, mcp_requests=()):
    expect = scenario.get("expect", {})
    failures = check_mcp(expect["mcp"], list(mcp_requests)) if "mcp" in expect else []
    if "max_request_gap_seconds" in expect:
        times = [request.get("_received_at_seconds") for request in requests]
        if len(times) < 2 or any(value is None for value in times):
            failures.append("missing provider receipt times for the shell deadline check")
        elif max(right - left for left, right in zip(times, times[1:])) > expect["max_request_gap_seconds"]:
            failures.append("tool execution exceeded the allowed model request interval")
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
    protocol = expect.get("protocol")
    if protocol and any(not r.get("_path", "").endswith("/" + protocol) for r in requests):
        failures.append(f"requests used the wrong wire protocol, expected {protocol}")
    sent = "\n".join(json.dumps(r) for r in requests)
    if "min_requests" in expect and len(requests) < expect["min_requests"]:
        failures.append(f"only {len(requests)} requests, expected at least {expect['min_requests']}")
    if expect.get("last_request_max_envelope_chars") and requests:
        request = {key: value for key, value in requests[-1].items() if not key.startswith("_")}
        reserve = request.get("max_output_tokens",request.get("max_tokens",request.get("max_completion_tokens",0))) or 0
        if len(json.dumps(request,ensure_ascii=False)) + reserve*4 > expect["last_request_max_envelope_chars"]:
            failures.append("final request did not fit the provider envelope")
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


def expand_workspace_paths(value, workspace):
    """Use absolute file aliases in black-box sandbox cases."""
    short = str(workspace)
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetShortPathNameW.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
        kernel.GetShortPathNameW.restype = wintypes.DWORD
        buffer = ctypes.create_unicode_buffer(32768)
        length = kernel.GetShortPathNameW(short, buffer, len(buffer))
        if not length or length >= len(buffer):
            raise OSError(ctypes.get_last_error(), "could not resolve workspace alias")
        short = buffer.value
    aliases = {"{workspace}": workspace.as_posix(), "{workspace_alias}": Path(short).as_posix()}

    def expand(item):
        if isinstance(item, str):
            for token, path in aliases.items():
                item = item.replace(token, path)
            return item
        if isinstance(item, list):
            return [expand(child) for child in item]
        if isinstance(item, dict):
            return {key: expand(child) for key, child in item.items()}
        return item

    return expand(value)


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
        script = expand_workspace_paths(scenario["script"], workspace) if scenario.get("workspace_paths") else scenario["script"]
        provider, port, log = start_provider(script, tmp, scenario.get("mcp"))
        env = isolated_env(tmp / "home", port)
        env.update(scenario.get("env", {}))
        if scenario.get("provider"):
            provider_id = scenario["provider"]
            env["OPENAI_API_KEY"] = "e2e-isolated-key"
            env["ANTHROPIC_API_KEY"] = "e2e-isolated-key"
            config = Path(env["NUR_HOME"]) / "config.toml"
            config.write_text(f'provider="{provider_id}"\nbase_url="http://127.0.0.1:{port}/v1"\nmodel="{scenario.get("model", "e2e-model")}"\n', encoding="utf-8")
        # Files under the scenario's nur home (hooks.toml, config additions).
        # "{home}" and "{workspace}" are replaced with the real paths.
        nur_home = Path(env["NUR_HOME"])
        for rel, text in scenario.get("setup_home_files", {}).items():
            text = text.replace("{home}", nur_home.as_posix()).replace("{workspace}", workspace.as_posix()).replace("{port}",str(port))
            (nur_home / rel).parent.mkdir(parents=True, exist_ok=True)
            (nur_home / rel).write_text(text, encoding="utf-8")
        setup_failures = []
        for command in scenario.get("setup_commands", []):
            # CLI steps a user runs first, e.g. `nur auth login --provider ...`.
            done = subprocess.run([str(binary), *command], env=env, cwd=workspace, stdin=subprocess.DEVNULL,
                                  capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
            if done.returncode != 0:
                setup_failures.append(f"setup `nur {' '.join(command)}` exited {done.returncode}: {done.stderr.strip()[:300]}")
        try:
            args = [str(binary), "--cwd", str(workspace), "run"]
            mode = scenario.get("mode", "auto")
            args += ["--yes"] if mode == "auto" else ["--mode", mode]
            args.append(scenario["prompt"])
            started = time.time()
            child = spawn_process(
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
            stop_tree(child)
        finally:
            stop_tree(provider)
        requests = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines() if line]
        index = tmp / "home" / ".nur" / "cache" / "skills-index.json"
        if index.exists() and not WARM_CACHE.exists():
            WARM_CACHE.mkdir(parents=True)
            shutil.copy(index, WARM_CACHE / index.name)
        mcp_log = tmp / "mcp.jsonl"
        mcp_requests = [json.loads(line) for line in mcp_log.read_text(encoding="utf-8").splitlines() if line] if mcp_log.exists() else []
        failures = setup_failures + check(scenario, result, workspace, requests, mcp_requests)
        files = sorted(str(p.relative_to(workspace)) for p in workspace.rglob("*") if p.is_file())
        shutil.copy(log, out / "requests.jsonl")
        if mcp_log.exists():
            shutil.copy(mcp_log, out / "mcp_requests.jsonl")
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
        unknown = sorted(set(opts.names) - {p.stem for p in paths})
        if unknown:
            parser.error("unknown scenarios: " + ", ".join(unknown))
        paths = [p for p in paths if p.stem in opts.names]
    if not paths:
        parser.error("no scenarios selected")
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
    skipped = sum(bool(r.get("skipped")) for r in reports)
    passed = len(reports) - failed - skipped
    print(f"\n{passed} passed / {failed} failed / {skipped} skipped · artifacts in {OUT}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
