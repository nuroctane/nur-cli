"""A slow MCP server in one ACP session must not block another server."""
import argparse, json, os, queue, subprocess, sys, threading, time
from fake_provider import LoopbackHTTPServer, State, make_handler
from run_e2e import OUT, find_binary, isolated_env, spawn_process, stop_tree


def run(binary):
    work = OUT / f'acp-mcp-concurrent-{time.time_ns()}'
    home, workspace = work / 'home', work / 'workspace'
    workspace.mkdir(parents=True)
    script = workspace / 'mcp.py'
    script.write_text("""import json,pathlib,sys,time
slow = sys.argv[1] == 'slow'
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 method=m['method']
 if method=='initialize': result={'protocolVersion':'2025-06-18','capabilities':{},'serverInfo':{'name':'test','version':'1'}}
 elif method=='tools/list': result={'tools':[{'name':'run','inputSchema':{'type':'object'}}]}
 elif method=='tools/call':
  if slow:
   pathlib.Path('slow_started').write_text('yes')
   time.sleep(8)
  result={'content':[{'type':'text','text':'done'}]}
 else: result={}
 print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':result}),flush=True)
""", encoding='utf-8')
    state = State([
        {'tool_calls':[{'name':'mcp','arguments':{'action':'call','server':'slow','tool':'run','arguments':{}}}]},
        {'tool_calls':[{'name':'mcp','arguments':{'action':'call','server':'fast','tool':'run','arguments':{}}}]},
        {'text':'Fast completed.'},
    ], work / 'requests.jsonl')
    server = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(state))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    with (home / '.nur/config.toml').open('a', encoding='utf-8') as config:
        config.write('\n[sandbox]\nmode = "off"\n')
    proc = spawn_process([str(binary),'acp'], cwd=workspace, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8', start_new_session=os.name != 'nt')
    incoming = queue.Queue()
    def read():
        for line in proc.stdout: incoming.put(json.loads(line))
        incoming.put({'eof':True})
    threading.Thread(target=read, daemon=True).start()
    def send(value):
        proc.stdin.write(json.dumps(value)+'\n'); proc.stdin.flush()
    def request(i, method, params): send({'jsonrpc':'2.0','id':i,'method':method,'params':params})
    def receive(i):
        while True:
            msg=incoming.get(timeout=20)
            assert not msg.get('eof'), msg
            if msg.get('method')=='session/request_permission':
                option=next(o['optionId'] for o in msg['params']['options'] if o['kind']=='allow_once')
                send({'jsonrpc':'2.0','id':msg['id'],'result':{'outcome':{'outcome':'selected','optionId':option}}})
            elif msg.get('id')==i:
                assert 'error' not in msg, msg
                return msg['result']
    try:
        request(1,'initialize',{'protocolVersion':1,'clientCapabilities':{}}); receive(1)
        for i,name in [(2,'slow'),(3,'fast')]:
            request(i,'session/new',{'cwd':str(workspace),'mcpServers':[{'name':name,'command':sys.executable,'args':[str(script),name],'env':[]}]})
            if name=='slow': slow=receive(i)['sessionId']
            else: fast=receive(i)['sessionId']
        request(4,'session/prompt',{'sessionId':slow,'prompt':[{'type':'text','text':'Call slow'}]})
        # Permit the slow call, then wait for its server to enter the request.
        deadline = time.monotonic() + 15
        while not (workspace / 'slow_started').exists():
            assert time.monotonic() < deadline, 'slow server did not start'
            try: msg=incoming.get(timeout=.1)
            except queue.Empty: continue
            assert not msg.get('eof'), msg
            if msg.get('method')=='session/request_permission':
                option=next(o['optionId'] for o in msg['params']['options'] if o['kind']=='allow_once')
                send({'jsonrpc':'2.0','id':msg['id'],'result':{'outcome':{'outcome':'selected','optionId':option}}})
        started=time.monotonic()
        request(5,'session/prompt',{'sessionId':fast,'prompt':[{'type':'text','text':'Call fast'}]})
        assert receive(5)['stopReason']=='end_turn'
        assert time.monotonic()-started < 4, 'one server blocked an independent ACP session'
        send({'jsonrpc':'2.0','method':'session/cancel','params':{'sessionId':slow}})
        assert receive(4)['stopReason']=='cancelled'
        print('PASS acp-mcp-concurrent')
    finally:
        stop_tree(proc); server.shutdown(); server.server_close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--bin')
    run(find_binary(parser.parse_args().bin))
