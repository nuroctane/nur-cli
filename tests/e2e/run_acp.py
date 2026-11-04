"""Black-box ACP session and permission flow against the real CLI."""
import argparse
import json
import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

from fake_provider import LoopbackHTTPServer, State, make_handler
from run_e2e import OUT, find_binary, isolated_env, spawn_process, stop_tree


def run(binary):
    work = OUT / f'acp-{time.time_ns()}'
    home, workspace = work / 'home', work / 'workspace'
    workspace.mkdir(parents=True)
    (workspace / 'scoped').mkdir()
    dap_fixture = json.loads((Path(__file__).parent / 'scenarios/160_debug_dap.json').read_text(encoding='utf-8'))
    (workspace / 'fake_dap.py').write_text(dap_fixture['setup_files']['fake_dap.py'], encoding='utf-8')
    (workspace / 'sample.py').write_text('pass\n', encoding='utf-8')
    mcp_script = workspace / 'client_mcp.py'
    mcp_script.write_text("""import json,os,sys,subprocess
for line in sys.stdin:
    msg=json.loads(line)
    if 'id' not in msg: continue
    method=msg['method']
    if method=='initialize': result={'protocolVersion':'2025-06-18','capabilities':{},'serverInfo':{'name':'test','version':'1'}}
    elif method=='tools/list': result={'tools':[{'name':'hello','inputSchema':{'type':'object'}}]}
    elif method=='tools/call':
        subprocess.Popen([sys.executable, '-c', "import pathlib,time;pathlib.Path('child_started').write_text('yes');time.sleep(4);pathlib.Path('escaped_child').write_text('bad')"], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        result={'content':[{'type':'text','text':'hello '+os.environ['CLIENT_TOKEN']+' '+os.environ['SHORT_TOKEN']+' true development 8080'}]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':msg['id'],'result':result}),flush=True)
""", encoding='utf-8')
    global_script = workspace / 'global_mcp.py'
    global_script.write_text("""import json,sys
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 if m['method']=='initialize': result={'protocolVersion':'2025-06-18','capabilities':{},'serverInfo':{'name':'global','version':'1'}}
 elif m['method']=='tools/list': result={'tools':[{'name':'global_hello','inputSchema':{'type':'object'}}]}
 else: result={}
 print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':result}),flush=True)
""", encoding='utf-8')
    state = State([
        {'text': 'First ACP response.'},
        {'text': 'Before tool.', 'tool_calls': [{'name': 'write_file', 'arguments': {'path': 'blocked.txt', 'contents': 'no'}}]},
        {'text': 'Denied as requested.'},
        {'tool_calls': [{'name': 'question', 'arguments': {'question': 'Choose a track', 'options': [{'label': 'Stable'}, {'label': 'Preview'}]}}]},
        {'text': 'Question answered.'},
        {'tool_calls': [{'name': 'write_file', 'arguments': {'path': 'cancelled.txt', 'contents': 'no'}}]},
        {'text': 'After cancellation.'},
        {'text': 'UNRELIABLE_PARTIAL', 'drop': 'mid_stream'},
        {'text': 'Stream recovered.'},
        {'tool_calls': [{'name':'debug','arguments':{'action':'start','session':'private','adapter':sys.executable,'adapter_args':['fake_dap.py'],'launch':{'program':'sample.py'}}}]},
        {'text':'Debugger started.'},
        {'tool_calls': [{'name':'debug','arguments':{'action':'request','session':'private','command':'stackTrace','arguments':{'threadId':1}}}]},
        {'text':'Isolation checked.'},
        {'tool_calls': [{'name':'debug','arguments':{'action':'stop','session':'private'}}]},
        {'text':'Debugger stopped.'},
        {'tool_calls': [{'name': 'mcp', 'arguments': {'action': 'list'}}]},
        {'tool_calls': [{'name': 'mcp', 'arguments': {'action': 'call', 'server': 'client-server', 'tool': 'hello', 'arguments': {}}}]},
        {'text': 'MCP used.'},
        {'tool_calls': [{'name': 'mcp', 'arguments': {'action': 'list'}}]},
        {'text': 'Other session checked.'},
        {'text': 'Scoped route used.'},
    ], work / 'requests.jsonl')
    server = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(state))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    with (home / '.nur/config.toml').open('a', encoding='utf-8') as config:
        config.write('\n[sandbox]\nmode = "off"\n')
        config.write(f'\n[[path_models]]\npath = "{(workspace / "scoped").as_posix()}"\nmodel = "scoped-e2e-model"\n')
        config.write(f'\n[mcp.servers.client-server]\ncommand = "{Path(sys.executable).as_posix()}"\nargs = ["{global_script.as_posix()}"]\nread_only = true\n')
    proc = spawn_process([str(binary), 'acp'], cwd=workspace, env=env, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         encoding='utf-8', start_new_session=os.name != 'nt')
    incoming = queue.Queue()
    permissions = []
    def read():
        for line in proc.stdout:
            try:
                incoming.put(json.loads(line))
            except ValueError:
                incoming.put({'invalid_stdout': line})
        incoming.put({'eof': True})
    threading.Thread(target=read, daemon=True).start()
    def send(message):
        proc.stdin.write(json.dumps(message) + '\n')
        proc.stdin.flush()
    def request(req_id, method, params):
        send({'jsonrpc': '2.0', 'id': req_id, 'method': method, 'params': params})
    def await_response(req_id, updates=None, cancel_session=None, allow=False):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            msg = incoming.get(timeout=max(.1, deadline - time.monotonic()))
            assert 'invalid_stdout' not in msg and 'eof' not in msg, msg
            if msg.get('method') == 'session/update':
                if updates is not None:
                    updates.append(msg)
            elif msg.get('method') == 'elicitation/create':
                assert msg['params']['message'] == 'Choose a track', msg
                send({'jsonrpc': '2.0', 'id': msg['id'], 'result': {'action': 'accept', 'content': {'answer': 'Preview'}}})
            elif msg.get('method') == 'session/request_permission':
                permissions.append(msg)
                call_id = msg['params']['toolCall']['toolCallId']
                if req_id == 4: assert 'Before tool.' in json.dumps(updates), 'assistant progress was held until the turn ended'
                assert updates is not None and any(update.get('params', {}).get('update', {}).get('toolCallId') == call_id for update in updates), 'permission did not reference an announced tool call'
                if cancel_session:
                    send({'jsonrpc': '2.0', 'method': 'session/cancel', 'params': {'sessionId': cancel_session}})
                    send({'jsonrpc': '2.0', 'id': msg['id'], 'result': {'outcome': {'outcome': 'cancelled'}}})
                else:
                    options = msg['params']['options']
                    selected = next(option['optionId'] for option in options if option['kind'] == ('allow_once' if allow else 'reject_once'))
                    send({'jsonrpc': '2.0', 'id': msg['id'], 'result': {'outcome': {'outcome': 'selected', 'optionId': selected}}})
            elif msg.get('id') == req_id:
                assert 'error' not in msg, msg
                return msg['result']
        raise AssertionError('ACP response timed out')
    try:
        request(1, 'initialize', {'protocolVersion': 1, 'clientCapabilities': {'elicitation': {'form': {}}}, 'clientInfo': {'name': 'nur-e2e', 'version': '1'}})
        assert await_response(1)['protocolVersion'] == 1
        request(2, 'session/new', {'cwd': str(workspace), 'mcpServers': []})
        session = await_response(2)['sessionId']
        updates = []
        request(3, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'First turn'}]})
        assert await_response(3, updates)['stopReason'] == 'end_turn'
        assert 'First ACP response.' in json.dumps(updates), updates
        request(4, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Write the file'}]})
        assert await_response(4, updates)['stopReason'] == 'end_turn'
        assert not (workspace / 'blocked.txt').exists()
        assert 'Denied as requested.' in json.dumps(updates), updates
        request(5, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Ask me'}]})
        assert await_response(5, updates)['stopReason'] == 'end_turn'
        assert 'Question answered.' in json.dumps(updates), updates
        requests = (work / 'requests.jsonl').read_text(encoding='utf-8')
        assert 'Preview' in requests and 'no interactive user' not in requests
        request(6, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Cancel this write'}]})
        assert await_response(6, updates, cancel_session=session)['stopReason'] == 'cancelled'
        assert not (workspace / 'cancelled.txt').exists()
        request(7, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Continue after cancellation'}]})
        assert await_response(7, updates)['stopReason'] == 'end_turn'
        assert 'After cancellation.' in json.dumps(updates), updates
        restart_start = len(updates)
        request(20, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Recover the stream'}]})
        assert await_response(20, updates)['stopReason'] == 'end_turn'
        recovered_updates = json.dumps(updates[restart_start:])
        assert 'Stream recovered.' in recovered_updates and 'UNRELIABLE_PARTIAL' not in recovered_updates, recovered_updates
        request(21, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Start a debugger'}]})
        assert await_response(21, updates, allow=True)['stopReason'] == 'end_turn'
        request(22, 'session/new', {'cwd': str(workspace), 'mcpServers': []})
        isolated = await_response(22)['sessionId']
        request(23, 'session/prompt', {'sessionId': isolated, 'prompt': [{'type': 'text', 'text': 'Inspect the other debugger'}]})
        assert await_response(23, updates, allow=True)['stopReason'] == 'end_turn'
        requests = [json.loads(line) for line in (work / 'requests.jsonl').read_text(encoding='utf-8').splitlines()]
        assert 'unknown session' in json.dumps(requests[-1]), 'debug session leaked across ACP sessions'
        request(24, 'session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'Stop my debugger'}]})
        assert await_response(24, updates, allow=True)['stopReason'] == 'end_turn'
        server_config = {'name': 'client-server', 'command': sys.executable, 'args': [str(mcp_script)], 'env': [{'name': 'CLIENT_TOKEN', 'value': 'client-secret-token'}, {'name': 'SHORT_TOKEN', 'value': 'z9'}, {'name': 'DEBUG', 'value': 'true'}, {'name': 'NODE_ENV', 'value': 'development'}, {'name': 'PORT', 'value': '8080'}]}
        request(8, 'session/new', {'cwd': str(workspace), 'mcpServers': [server_config]})
        scoped = await_response(8)['sessionId']
        before_permission = len(permissions)
        request(9, 'session/prompt', {'sessionId': scoped, 'prompt': [{'type': 'text', 'text': 'Use the client server'}]})
        assert await_response(9, updates, allow=True)['stopReason'] == 'end_turn'
        assert len(permissions) > before_permission, 'scoped server inherited global read-only approval'
        assert 'MCP used.' in json.dumps(updates), updates
        requests = (work / 'requests.jsonl').read_text(encoding='utf-8')
        assert 'hello [secret:' in requests and 'client-secret-token' not in requests and 'z9' not in requests
        assert 'true development 8080' in requests, 'ordinary environment settings corrupted MCP output'
        request(10, 'session/new', {'cwd': str(workspace), 'mcpServers': []})
        other = await_response(10)['sessionId']
        request(11, 'session/prompt', {'sessionId': other, 'prompt': [{'type': 'text', 'text': 'List my servers'}]})
        assert await_response(11, updates)['stopReason'] == 'end_turn'
        requests = [json.loads(line) for line in (work / 'requests.jsonl').read_text(encoding='utf-8').splitlines()]
        assert 'global_hello' in json.dumps(requests[-1]) and 'client-secret-token' not in json.dumps(requests[-1]), 'other session saw scoped MCP state'
        request(12, 'session/new', {'cwd': str(workspace / 'scoped'), 'mcpServers': []})
        scoped_route = await_response(12)['sessionId']
        request(13, 'session/prompt', {'sessionId': scoped_route, 'prompt': [{'type': 'text', 'text': 'Use my project route'}]})
        assert await_response(13, updates)['stopReason'] == 'end_turn'
        requests = [json.loads(line) for line in (work / 'requests.jsonl').read_text(encoding='utf-8').splitlines()]
        assert requests[-1].get('model') == 'scoped-e2e-model', requests[-1].get('model')
        deadline = time.monotonic() + 5
        while not (workspace / 'child_started').exists() and time.monotonic() < deadline:
            time.sleep(.05)
        assert (workspace / 'child_started').exists(), 'MCP child did not start'
        proc.stdin.close()
        proc.wait(timeout=10)
        time.sleep(4.5)
        assert not (workspace / 'escaped_child').exists(), 'MCP descendant survived ACP shutdown'
        print('PASS acp-session')
    finally:
        stop_tree(proc)
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    run(find_binary(parser.parse_args().bin))
