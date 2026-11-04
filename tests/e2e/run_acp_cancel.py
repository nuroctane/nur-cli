"""Cancelling an in-flight ACP MCP call must not leave its server running."""
import argparse
import json
import os
import queue
import subprocess
import sys
import threading
import time

from fake_provider import FakeMcp, LoopbackHTTPServer, State, make_handler
from run_e2e import OUT, find_binary, isolated_env, spawn_process, stop_tree


def run(binary, http=False):
    work = OUT / f'acp-cancel-mcp-{time.time_ns()}'
    home, workspace = work / 'home', work / 'workspace'
    workspace.mkdir(parents=True)
    script = workspace / 'slow_mcp.py'
    script.write_text("""import json,pathlib,sys,time
for line in sys.stdin:
    m=json.loads(line)
    if 'id' not in m: continue
    method=m['method']
    if method=='initialize': result={'protocolVersion':'2025-06-18','capabilities':{},'serverInfo':{'name':'test','version':'1'}}
    elif method=='tools/list': result={'tools':[{'name':'slow','inputSchema':{'type':'object'}}]}
    elif method=='tools/call':
        pathlib.Path('started').write_text('yes')
        time.sleep(8)
        pathlib.Path('escaped').write_text('bad')
        result={'content':[{'type':'text','text':'done'}]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':result}),flush=True)
""", encoding='utf-8')
    state = State([{'tool_calls': [{'name': 'mcp', 'arguments': {'action': 'call', 'server': 'slow', 'tool': 'slow', 'arguments': {}}}]}], work / 'requests.jsonl')
    mcp = FakeMcp({'key':'http-test-token', 'delay_call_seconds':8, 'tools':[{'name':'slow','inputSchema':{'type':'object'}}], 'results':{'slow':{'content':[{'type':'text','text':'done'}]}}}, work / 'mcp.jsonl') if http else None
    server = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(state, mcp))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    with (home / '.nur/config.toml').open('a', encoding='utf-8') as config:
        config.write('\n[sandbox]\nmode = "off"\n')
        if http:
            config.write(f'\n[mcp.servers.slow]\nurl = "http://127.0.0.1:{server.server_port}/mcp"\nbearer_env = "NUR_TEST_HTTP_TOKEN"\n')
            env['NUR_TEST_HTTP_TOKEN'] = 'http-test-token'
    proc = spawn_process([str(binary), 'acp'], cwd=workspace, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8', start_new_session=os.name != 'nt')
    incoming = queue.Queue()
    def read():
        for line in proc.stdout:
            incoming.put(json.loads(line))
        incoming.put({'eof': True})
    threading.Thread(target=read, daemon=True).start()
    def send(value):
        proc.stdin.write(json.dumps(value) + '\n')
        proc.stdin.flush()
    def response(req_id):
        while True:
            msg = incoming.get(timeout=20)
            assert not msg.get('eof'), msg
            if msg.get('method') == 'session/request_permission':
                option = next(o['optionId'] for o in msg['params']['options'] if o['kind'] == 'allow_once')
                send({'jsonrpc':'2.0','id':msg['id'],'result':{'outcome':{'outcome':'selected','optionId':option}}})
            elif msg.get('id') == req_id:
                assert 'error' not in msg, msg
                return msg['result']
    def request(req_id, method, params):
        send({'jsonrpc':'2.0','id':req_id,'method':method,'params':params})
    try:
        request(1, 'initialize', {'protocolVersion':1,'clientCapabilities':{}})
        response(1)
        request(2, 'session/new', {'cwd':str(workspace),'mcpServers':[] if http else [{'name':'slow','command':sys.executable,'args':[str(script)],'env':[]}]})
        session = response(2)['sessionId']
        request(3, 'session/prompt', {'sessionId':session,'prompt':[{'type':'text','text':'Call slow'}]})
        def cancel_when_started():
            deadline = time.monotonic() + 15
            while not (workspace / 'started').exists() and time.monotonic() < deadline:
                time.sleep(.05)
            if (workspace / 'started').exists():
                send({'jsonrpc':'2.0','method':'session/cancel','params':{'sessionId':session}})
        threading.Thread(target=cancel_when_started, daemon=True).start()
        started = time.monotonic()
        assert response(3)['stopReason'] == 'cancelled'
        assert time.monotonic() - started < 6, 'in-flight MCP cancellation was not prompt'
        proc.stdin.close()
        proc.wait(timeout=5)
        time.sleep(1)
        if not http:
            assert not (workspace / 'escaped').exists(), 'cancelled MCP server continued work'
        print('PASS acp-' + ('http' if http else 'stdio') + '-mcp-cancellation')
    finally:
        stop_tree(proc)
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    run(find_binary(args.bin), args.http)
