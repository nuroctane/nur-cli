"""Drive the real gateway with a loopback Telegram API and scripted provider."""
import argparse
import json
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler

from fake_provider import LoopbackHTTPServer, State, make_handler
from run_e2e import OUT, find_binary, isolated_env, spawn_process, stop_tree


def wait_for(predicate, what, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError(f'timed out waiting for {what}')


def run(binary):
    work = OUT / f'gateway-inbox-{time.time_ns()}'
    home, workspace = work / 'home', work / 'workspace'
    workspace.mkdir(parents=True)
    log = work / 'requests.jsonl'
    script = [
        {'tool_calls': [{'name': 'question', 'arguments': {'question': 'Choose the release track',
                        'options': [{'label': 'Stable'}, {'label': 'Preview'}]}}]},
        {'text': 'First turn finished.'},
        {'text': 'Follow-up finished.'},
        {'tool_calls': [{'name': 'secscan', 'arguments': {'action': 'baseline', 'ids': ['a1b2c3d4e5f6'], 'reason': 'fixture'}}]},
        {'text': 'Baseline was denied.'},
        {'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import pathlib,time;pathlib.Path(\'started.txt\').write_text(\'yes\');time.sleep(60)"'}}]},
        {'text': 'After cancellation.'},
        {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'Async writer', 'prompt': 'Run the fixture and write the file.', 'subagent_type': 'general', 'async': True}}]},
        {'match_child': True, 'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time;time.sleep(3)"'}}]},
        {'match_child': False, 'text': 'Child launched.'},
        {'match_child': True, 'tool_calls': [{'name': 'write_file', 'arguments': {'path': 'child.txt', 'content': 'not authorized'}}]},
        {'match_child': True, 'text': 'Child acknowledged denial.'},
        {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'Async question', 'prompt': 'Ask the user which track.', 'subagent_type': 'general', 'async': True}}]},
        {'match_child': True, 'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time;time.sleep(3)"'}}]},
        {'match_child': False, 'text': 'Question child launched.'},
        {'match_child': True, 'tool_calls': [{'name': 'question', 'arguments': {'question': 'Child needs a track', 'options': [{'label': 'Stable'}, {'label': 'Preview'}]}}]},
        {'match_child': True, 'text': 'Child received the answer.'},
        {'match_child': False, 'tool_calls': [{'name': 'secret', 'arguments': {'action': 'request', 'name': 'GATEWAY_TEST_TOKEN', 'purpose': 'access the fixture'}}]},
        {'match_child': False, 'text': 'Credential needed.'},
        {'match_child': False, 'tool_calls': [{'name': 'secscan', 'arguments': {'action': 'baseline', 'ids': ['a1b2c3d4e5f6'], 'reason': 'fixture accepted'}}]},
        {'match_child': False, 'text': 'Baseline was accepted.'},
        {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'Missing provider', 'prompt': 'Inspect briefly.', 'provider': 'deepseek', 'subagent_type': 'general', 'async': True}}]},
        {'match_child': False, 'text': 'Provider child launched.'},
    ]
    provider_state = State(script, log)
    provider = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(provider_state))
    updates, sent, offsets, lock = [{'update_id': 1, 'my_chat_member': {}}], [], [], threading.Lock()

    def queue(text, sender=55):
        with lock:
            updates.append({'update_id': len(updates) + 1, 'message': {
                'chat': {'id': -100}, 'from': {'id': sender}, 'text': text}})

    class Telegram(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, obj):
            body = json.dumps(obj).encode()
            try:
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

        def do_GET(self):
            from urllib.parse import parse_qs, urlsplit
            query = parse_qs(urlsplit(self.path).query)
            offset = int(query.get('offset', ['0'])[0])
            with lock:
                offsets.append(offset)
                batch = [u for u in updates if u['update_id'] >= offset]
            if not batch:
                time.sleep(.1)
            self.reply({'ok': True, 'result': batch})

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            with lock:
                sent.append(body.get('text', ''))
            self.reply({'ok': True, 'result': {}})

    telegram = LoopbackHTTPServer(('127.0.0.1', 0), Telegram)
    for server in (provider, telegram):
        threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, provider.server_port)
    env.pop('DEEPSEEK_API_KEY', None)
    env.update({'NUR_GATEWAY_API_URL': f'http://127.0.0.1:{telegram.server_port}',
                'HTTPS_PROXY': 'http://127.0.0.1:1', 'https_proxy': 'http://127.0.0.1:1',
                'NO_PROXY': '127.0.0.1,localhost'})
    (home / '.nur/config.toml').write_text(
        f'provider = "vllm"\nmodel = "e2e-model"\nbase_url = "http://127.0.0.1:{provider.server_port}/v1"\n'
        'ecosystem_auto_ensure = false\nauto_update = false\nnative_memory = false\n'
        '[typesafe]\nenabled = false\n[sandbox]\nmode = "off"\n', encoding='utf-8')
    (workspace / '.nur').mkdir()
    (workspace / '.nur/permissions.toml').write_text('ask = ["write_file"]\n', encoding='utf-8')
    proc = spawn_process([str(binary), 'gateway', '--token', 'test-token', '--chat=-100', '--user=55'],
                         cwd=workspace, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for(lambda: 2 in offsets, 'offset after a non-text update')
        queue('Choose a release track')
        wait_for(lambda: any('Choose the release track' in s and '/answer' in s for s in sent), 'question ticket')
        queue('/inbox')
        wait_for(lambda: sum('Choose the release track' in s and '/answer' in s for s in sent) >= 2, 'inbox listing')
        queue('/answer 1 9')
        wait_for(lambda: any('valid option' in s for s in sent), 'invalid answer rejection')
        assert provider_state.count == 1, 'invalid answer advanced the turn'
        queue('Follow-up prompt')
        queue('/answer 1 2', sender=56)
        time.sleep(1)
        assert provider_state.count == 1, 'unauthorized answer advanced the turn'
        queue('/answer 1 2')
        wait_for(lambda: any('First turn finished.' in s for s in sent), 'first reply')
        wait_for(lambda: any('Follow-up finished.' in s for s in sent), 'queued follow-up')
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Preview' in json.dumps(requests[1]), 'selected answer was not delivered to the model'
        assert 'Follow-up prompt' in json.dumps(requests[2]), 'ordinary message was not queued'
        queue('Baseline the fixture finding')
        wait_for(lambda: any('Approval:' in s and '/answer 2' in s for s in sent), 'forced approval ticket')
        queue('/answer 2 deny')
        wait_for(lambda: any('Baseline was denied.' in s for s in sent), 'denial result')
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'user denied this tool call' in json.dumps(requests[4]), 'denial did not reach the model'
        assert not (workspace / '.nur/secscan-baseline.json').exists(), 'denied baseline was written'
        queue('Start the long fixture')
        wait_for(lambda: (workspace / 'started.txt').exists(), 'long-running command')
        queue('/cancel')
        wait_for(lambda: any('Cancelling the current turn' in s for s in sent), 'cancellation acknowledgement')
        queue('Continue after cancellation')
        wait_for(lambda: any('After cancellation.' in s for s in sent), 'turn after cancellation')
        queue('Start an asynchronous writer')
        wait_for(lambda: any('Child launched.' in s for s in sent), 'asynchronous parent reply')
        wait_for(lambda: any('Subagent Approval:' in s and '/answer 3' in s for s in sent), 'asynchronous child approval')
        queue('/answer 3 deny')
        wait_for(lambda: any('user denied this tool call' in line and '[SUBAGENT:' in line for line in log.read_text(encoding='utf-8').splitlines()), 'child continuation')
        assert not (workspace / 'child.txt').exists(), 'denied child write occurred'
        queue('Ask an asynchronous child to choose a track')
        wait_for(lambda: any('Question child launched.' in s for s in sent), 'question child parent reply')
        wait_for(lambda: any('Subagent - Question:' in s and 'Child needs a track' in s and '/answer 4' in s for s in sent), 'asynchronous child question')
        queue('/answer 4 2')
        wait_for(lambda: any('Child needs a track' in line and 'Preview' in line and '[SUBAGENT:' in line and 'user selected' in line for line in log.read_text(encoding='utf-8').splitlines()), 'child question answer')
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        child_requests = [r for r in requests if 'Ask the user which track' in json.dumps(r)
                          and '[SUBAGENT:' in json.dumps(r)
                          and not any(t.get('function', {}).get('name') == 'agent' for t in r.get('tools', []))]
        assert child_requests and all(any(t.get('function', {}).get('name') == 'question' for t in r.get('tools', [])) for r in child_requests), 'child did not receive the question tool'
        queue('Request the fixture credential')
        wait_for(lambda: any('GATEWAY_TEST_TOKEN' in s and 'nur secrets set' in s for s in sent), 'local credential instruction')
        wait_for(lambda: any('Credential needed.' in s for s in sent), 'credential request result')
        queue('Accept the fixture finding')
        wait_for(lambda: any('Approval:' in s and '/answer 5' in s for s in sent), 'approved baseline ticket')
        queue('/answer 5 approve')
        wait_for(lambda: any('Baseline was accepted.' in s for s in sent), 'approved baseline result')
        baseline = json.loads((workspace / '.nur/secscan-baseline.json').read_text(encoding='utf-8'))
        assert any(row['id'] == 'a1b2c3d4e5f6' and row['reason'] == 'fixture accepted' for row in baseline['accepted']), 'approved baseline was not recorded'
        queue('Start the provider child')
        wait_for(lambda: any('DeepSeek' in s and 'nur auth login --provider deepseek' in s for s in sent), 'missing provider notice')
        print('PASS gateway-inbox', flush=True)
    finally:
        stop_tree(proc)
        for server in (provider, telegram):
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    run(find_binary(parser.parse_args().bin))
