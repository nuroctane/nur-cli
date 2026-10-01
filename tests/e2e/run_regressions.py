"""Black-box regressions for account locks, session ownership and cancellation.

All servers, credentials, bridge processes and Nur state belong to throwaway
homes. Requires pyte; uses the same real-terminal driver as run_startup.py.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from fake_provider import State, make_handler
from run_e2e import find_binary, isolated_env
from run_startup import BuilderLease, Terminal

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'target' / 'regression-e2e'


def wait_for(predicate, timeout=10):
    end = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < end, 'condition did not complete'
        time.sleep(.02)


def fixture(name, port=1):
    work = OUT / f'{name}-{time.time_ns()}'
    workspace, home = work / 'workspace', work / 'home'
    workspace.mkdir(parents=True)
    env = isolated_env(home, port)
    env['TERM'] = 'xterm-256color'
    env['NUR_DISABLE_NATIVE_MEMORY'] = '1'
    env['NUR_STARTUP_TRACE'] = str(work / 'startup.jsonl')
    config(home, port)
    return work, workspace, home, env


def config(home, port, extra='', provider='vllm', model='e2e-model'):
    (home / '.nur/config.toml').write_text(
        f'provider={json.dumps(provider)}\nmodel={json.dumps(model)}\n'
        f'base_url="http://127.0.0.1:{port}/v1"\ntheme="gold"\n'
        'auto_update=false\necosystem_auto_ensure=false\nnative_memory=false\n' +
        (extra or '[typesafe]\nenabled=false\n'), encoding='utf-8')


def server(handler):
    instance = ThreadingHTTPServer(('127.0.0.1', 0), handler)
    threading.Thread(target=instance.serve_forever, daemon=True).start()
    return instance


def terminal_log(term, work):
    if term:
        (work / 'terminal.txt').write_text(term.output, encoding='utf-8')
        term.close()


def queue_reset(binary):
    work, workspace, home, env = fixture('queue-reset')
    state = State([{'text': 'FRESH_SESSION_REPLY_6612'}], work / 'requests.jsonl')
    provider = server(make_handler(state))
    config(home, provider.server_port)
    lease = BuilderLease(home / '.nur/cache/skills-index.lock')
    term = None
    try:
        term = Terminal(binary, workspace, env)
        term.until('F6 inspect', 5)
        wait_for(lambda: (work / 'startup.jsonl').exists() and 'session_saved' in (work / 'startup.jsonl').read_text())
        term.type_text('OBSOLETE_QUEUED_PROMPT_7718')
        term.write('\r')
        term.until('OBSOLETE_QUEUED_PROMPT_7718')
        term.type_text('/new')
        term.write('\r')
        term.until('new session')
        assert state.count == 0
        lease.release()
        term.until('skills ready', 45, found=lambda: 'skills_ready' in (work / 'startup.jsonl').read_text())
        term.type_text('FRESH_SESSION_PROMPT_6621')
        term.write('\r')
        term.until('FRESH_SESSION_REPLY_6612', 15)
        requests = (work / 'requests.jsonl').read_text(encoding='utf-8')
        assert state.count == 1, 'a hidden old request ran after reset'
        assert 'OBSOLETE_QUEUED_PROMPT_7718' not in requests
        assert 'FRESH_SESSION_PROMPT_6621' in requests
    finally:
        lease.release()
        terminal_log(term, work)
        provider.shutdown(); provider.server_close()


def external_skill_refresh(binary):
    work, workspace, home, env = fixture('external-skill-refresh')
    state = State([{'text':'REFRESHED_SKILL_REPLY_1726'}], work / 'requests.jsonl')
    provider = server(make_handler(state)); config(home, provider.server_port)
    parent = home / '.nur/skills/refresh-pack/nested'
    parent.mkdir(parents=True)
    (parent / 'seed').mkdir()
    (parent / 'seed/SKILL.md').write_text('---\nname: refresh-seed\ndescription: Fixture\n---\nSeed.', encoding='utf-8')
    refreshed = subprocess.run([str(binary), 'ecosystem', 'refresh-skills'], cwd=workspace,
                               env=env, capture_output=True, text=True, timeout=60)
    assert refreshed.returncode == 0, refreshed.stdout + refreshed.stderr
    snapshot = json.loads((home / '.nur/cache/skills-index.json').read_text(encoding='utf-8'))
    assert any(skill['name'] == 'refresh-seed' for skill in snapshot['skills'])
    term, lease = None, None
    try:
        term = Terminal(binary,workspace,env); term.until('F6 inspect',5)
        term.until('prepared',60,found=lambda: (work / 'startup.jsonl').exists() and '"ready"' in (work / 'startup.jsonl').read_text())
        cache = home / '.nur/cache/skills-index.json'
        previous = json.loads(cache.read_text(encoding='utf-8'))
        new_skill = parent / 'added-later'; new_skill.mkdir()
        (new_skill / 'SKILL.md').write_text('---\nname: zz-external-refresh\ndescription: External refresh fixture\n---\nEXTERNAL_SKILL_BODY_1773',encoding='utf-8')
        lease = BuilderLease(home / '.nur/cache/skills-index.lock')
        term.type_text('/skills'); term.write('\r')
        term.type_text('/added-later Please use this fixture'); term.write('\r')
        term.type_text('EDITABLE_DURING_REFRESH_9921')
        assert state.count == 0
        assert json.loads(cache.read_text(encoding='utf-8')) == previous, 'refresh deleted the previous complete snapshot'
        lease.release()
        term.until('REFRESHED_SKILL_REPLY_1726',60)
        assert 'EXTERNAL_SKILL_BODY_1773' in (work / 'requests.jsonl').read_text(encoding='utf-8')
    finally:
        if lease: lease.release()
        terminal_log(term,work); provider.shutdown(); provider.server_close()


def account_lock(binary, action):
    work, workspace, home, env = fixture('account-' + action)
    helpers = work / 'helpers'; helpers.mkdir()
    helper = helpers / 'refresh.py'
    helper.write_text('import pathlib,time\n' + f'p=pathlib.Path({str(work)!r})\n'
        '(p/"entered").write_text("refresh")\nend=time.monotonic()+40\n'
        'while not (p/"release").exists() and time.monotonic()<end: time.sleep(.02)\n'
        'print("synthetic-refreshed-token")\n', encoding='utf-8')
    if os.name == 'nt':
        (helpers / 'gh.cmd').write_text(f'@echo off\r\n"{sys.executable}" "{helper}"\r\n', encoding='utf-8')
    else:
        wrapper = helpers / 'gh'
        wrapper.write_text(f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(helper))}\n', encoding='utf-8')
        wrapper.chmod(0o700)
    env['PATH'] = str(helpers) + os.pathsep + env.get('PATH', '')
    config(home, 1, provider='github-copilot', model='gpt-4o')
    (home / '.nur/auth.json').write_text(json.dumps({
        'api_key':'synthetic-expired-token', 'provider':'github-copilot',
        'source':'oauth', 'auth_method':'oauth', 'refresh_token':'gh', 'expires_at':1,
    }), encoding='utf-8')
    term = None
    try:
        term = Terminal(binary, workspace, env)
        term.until('F6 inspect', 5)
        wait_for(lambda: (work / 'entered').exists(), 5)
        if action == 'logout':
            term.type_text('/logout'); term.write('\r')
        elif action == 'key':
            term.type_text('/login openai'); term.write('\r')
            term.until('how to sign in', 5)
            # The API-key method is the second route after browser auth.
            term.write('\x1b[B'); term.pump(.2); term.write('\r')
            term.until('API key', 5)
            for char in 'synthetic-key': term.write(char); term.pump(.025)
            term.write('\r'); term.pump(.1)
            term.write('\x1b'); term.pump(.2); term.write('\x1b'); term.pump(.2)
            term.write('\x1b'); term.pump(.2)
            term.until('login dismissed', 5, found=lambda: all(
                marker not in '\n'.join(term.screen.display)
                for marker in ['choose a provider', 'how to sign in', 'API key']))
        elif action == 'model':
            term.type_text('/model chosen-model'); term.write('\r')
            term.until('model → chosen-model', 5)
        elif action == 'delete':
            term.type_text('/auth'); term.write('\r')
            term.until('auth vault', 5)
            for char in 'github': term.write(char); term.pump(.025)
            term.write('\x1b[3~'); term.pump(.2); term.write('\x1b'); term.pump(.2)
        term.type_text('ACCOUNT_WORKER_STILL_EDITABLE_7218')
        assert not (work / 'release').exists()
        # Quit must not wait for the store lease or the still-blocked refresh.
        term.write('\x15'); term.pump(.2)
        term.type_text('/quit'); term.write('\r')
        wait_for(lambda: not term.proc.isalive() if os.name == 'nt' else term.proc.poll() is not None, 5)
    finally:
        (work / 'release').write_text('go')
        terminal_log(term, work)


def cancelled_judgment(binary, provider_name='vllm', model='e2e-model'):
    work, workspace, home, env = fixture('cancel-' + provider_name)
    entered, release = threading.Event(), threading.Event()
    counts = []
    class Judge(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_POST(self):
            self.rfile.read(int(self.headers['content-length']))
            counts.append(time.monotonic()); entered.set(); release.wait(30)
            reply = b'{"answers":{},"model":"fake-judge"}'
            try:
                self.send_response(503); self.send_header('Content-Length', str(len(reply))); self.end_headers(); self.wfile.write(reply)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError): pass
    judge = server(Judge)
    state = State([{'tool_calls':[{'name':'question', 'arguments':{'question':'STALE_QUESTION_8512', 'options':[{'label':'Node'}, {'label':'Bun'}]}}]}, {'text':'NEW_TURN_REPLY_8221'}], work / 'requests.jsonl')
    provider = server(make_handler(state))
    config(home, provider.server_port, f'[typesafe]\nenabled=true\nbase_url="http://127.0.0.1:{judge.server_port}/v1/systemone"\ntimeout_ms=20000\nretries=3\n', provider_name, model)
    env['OPENAI_API_KEY'] = env['ANTHROPIC_API_KEY'] = 'synthetic-key'
    term = None
    try:
        term = Terminal(binary, workspace, env)
        term.until('F6 inspect', 5)
        term.type_text('Ask which runtime I prefer'); term.write('\r')
        term.until('judgment started', 45, found=entered.is_set)
        term.write('\x1b')
        # Drain cancellation completion before asking for a new session.
        end = time.monotonic() + 1
        while time.monotonic() < end: term.pump(.05)
        term.type_text('/new'); term.write('\r'); term.until('new session', 3)
        assert 'STALE_QUESTION_8512' not in '\n'.join(term.screen.display)
        requests_at_cancel = len(counts)
        release.set()
        end = time.monotonic() + .8
        while time.monotonic() < end: term.pump(.05)
        assert len(counts) == requests_at_cancel, 'cancelled judgment made a later retry'
        assert 'STALE_QUESTION_8512' not in '\n'.join(term.screen.display)
        assert not (workspace / 'should-not-exist').exists()
    finally:
        release.set(); terminal_log(term, work)
        provider.shutdown(); provider.server_close(); judge.shutdown(); judge.server_close()


def bridge_ownership(binary):
    work, workspace, home, env = fixture('bridge-ownership')
    helper = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])
    directory = home / '.nur/jev'; directory.mkdir(exist_ok=True)
    def run(*args):
        result = subprocess.run([str(binary), 'jev', *args], env=env, cwd=workspace, capture_output=True, text=True, encoding='utf-8', timeout=40)
        assert result.returncode == 0, result.stderr
        return result
    try:
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0)); port = s.getsockname()[1]
        (directory / 'bridge.json').write_text(json.dumps({'pid':helper.pid, 'port':port, 'backend':'mock', 'endpoint':f'http://127.0.0.1:{port}/v1/systemone', 'started_at':'2000-01-01T00:00:00Z'}), encoding='utf-8')
        run('stop')
        assert helper.poll() is None, 'stale record killed an unrelated helper'
        run('start', '--backend', 'mock', '--port', str(port))
        saved = json.loads((directory / 'bridge.json').read_text())
        assert saved['process_identity'] and saved['instance']
        run('start', '--backend', 'mock', '--port', str(port))
        assert json.loads((directory / 'bridge.json').read_text()) == saved, 'idempotent start replaced the owned instance'
        import urllib.request
        health_url = f'http://127.0.0.1:{port}/health'
        health = json.load(urllib.request.urlopen(health_url, timeout=2))
        assert health['pid'] == saved['pid'] and health['instance'] == saved['instance']
        try:
            urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/shutdown', data=b'', headers={'X-Nur-Bridge-Instance':'wrong'}), timeout=2)
            raise AssertionError('wrong nonce shut down the bridge')
        except urllib.error.HTTPError as error: assert error.code == 403
        run('stop')
        assert not (directory / 'bridge.json').exists()
        def port_closed():
            with socket.socket() as s: return s.connect_ex(('127.0.0.1', port)) != 0
        wait_for(port_closed, 5)
        assert helper.poll() is None
    finally:
        if (directory / 'bridge.json').exists(): run('stop')
        if helper.poll() is None: helper.terminate()
        helper.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin'); parser.add_argument('cases', nargs='*')
    args = parser.parse_args(); binary = find_binary(args.bin).resolve()
    cases = {'queue-reset':queue_reset, 'bridge-ownership':bridge_ownership, 'external-skill-refresh':external_skill_refresh}
    cases.update({f'account-{action}':lambda b, action=action: account_lock(b, action) for action in ['logout','key','model','delete']})
    cases.update({f'cancel-{provider}':lambda b, p=provider, m=model: cancelled_judgment(b, p, m) for provider, model in [('vllm','e2e-model'), ('openai','gpt-5.5'), ('anthropic','claude-sonnet-5')]})
    unknown = set(args.cases) - cases.keys()
    if unknown: parser.error('unknown cases: ' + ', '.join(sorted(unknown)))
    results = []
    for name in args.cases or cases:
        try:
            cases[name](binary); results.append({'case':name, 'passed':True}); print('PASS ' + name, flush=True)
        except Exception as error:
            results.append({'case':name, 'passed':False, 'error':str(error)}); print('FAIL ' + name + ': ' + str(error), flush=True)
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'report.json').write_text(json.dumps(results, indent=2), encoding='utf-8')
    return 0 if results and all(r['passed'] for r in results) else 1


if __name__ == '__main__': sys.exit(main())
