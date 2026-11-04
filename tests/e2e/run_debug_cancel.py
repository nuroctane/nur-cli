"""A blocked debugger adapter must stop promptly when its ACP turn is cancelled."""
import argparse,json,os,queue,subprocess,sys,threading,time
from fake_provider import LoopbackHTTPServer,State,make_handler
from run_e2e import OUT,find_binary,isolated_env,spawn_process,stop_tree

def run(binary):
 work=OUT/f'debug-cancel-{time.time_ns()}';home=work/'home';workspace=work/'workspace';workspace.mkdir(parents=True)
 (workspace/'program.py').write_text('pass\n',encoding='utf-8')
 adapter=workspace/'adapter.py'
 adapter.write_text("""import json,pathlib,sys,time
seq=1;pending=None
def send(value):
 global seq
 value['seq']=seq;seq+=1
 data=json.dumps(value).encode();sys.stdout.buffer.write(f'Content-Length: {len(data)}\\r\\n\\r\\n'.encode()+data);sys.stdout.buffer.flush()
def response(req,body=None):send({'type':'response','request_seq':req['seq'],'command':req['command'],'success':True,'body':body or {}})
while True:
 headers={}
 while True:
  line=sys.stdin.buffer.readline()
  if not line: sys.exit(0)
  if line in (b'\\r\\n',b'\\n'): break
  k,v=line.decode().split(':',1);headers[k.lower()]=v.strip()
 n=int(headers['content-length'])
 if n>100000:
  pathlib.Path('adapter_blocked').write_text('yes');time.sleep(8)
 req=json.loads(sys.stdin.buffer.read(n))
 if req['command']=='initialize': response(req,{'supportsConfigurationDoneRequest':True})
 elif req['command']=='launch': pending=req;send({'type':'event','event':'initialized'})
 elif req['command']=='configurationDone':response(req);response(pending)
 else:response(req)
""",encoding='utf-8')
 state=State([
  {'tool_calls':[{'name':'debug','arguments':{'action':'start','session':'fixture','adapter':sys.executable,'adapter_args':[str(adapter)],'launch':{'program':'program.py'}}}]},
  {'tool_calls':[{'name':'debug','arguments':{'action':'request','session':'fixture','command':'evaluate','arguments':{'expression':'x'*200000}}}]},
 ],work/'requests.jsonl')
 server=LoopbackHTTPServer(('127.0.0.1',0),make_handler(state));threading.Thread(target=server.serve_forever,daemon=True).start()
 env=isolated_env(home,server.server_port)
 with (home/'.nur/config.toml').open('a',encoding='utf-8') as config:config.write('\n[sandbox]\nmode = "off"\n')
 proc=spawn_process([str(binary),'acp'],cwd=workspace,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,encoding='utf-8',start_new_session=os.name!='nt')
 incoming=queue.Queue()
 def read():
  for line in proc.stdout:incoming.put(json.loads(line))
  incoming.put({'eof':True})
 threading.Thread(target=read,daemon=True).start()
 def send(msg):proc.stdin.write(json.dumps(msg)+'\n');proc.stdin.flush()
 def request(i,method,params):send({'jsonrpc':'2.0','id':i,'method':method,'params':params})
 def response(i):
  while True:
   msg=incoming.get(timeout=20);assert not msg.get('eof'),msg
   if msg.get('method')=='session/request_permission':
    choice=next(o['optionId'] for o in msg['params']['options'] if o['kind']=='allow_once')
    send({'jsonrpc':'2.0','id':msg['id'],'result':{'outcome':{'outcome':'selected','optionId':choice}}})
   elif msg.get('id')==i:
    assert 'error' not in msg,msg
    return msg['result']
 try:
  request(1,'initialize',{'protocolVersion':1,'clientCapabilities':{}});response(1)
  request(2,'session/new',{'cwd':str(workspace),'mcpServers':[]});session=response(2)['sessionId']
  request(3,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'Debug the fixture'}]})
  def cancel_on_block():
   deadline=time.monotonic()+15
   while not (workspace/'adapter_blocked').exists() and time.monotonic()<deadline:time.sleep(.05)
   if (workspace/'adapter_blocked').exists():send({'jsonrpc':'2.0','method':'session/cancel','params':{'sessionId':session}})
  threading.Thread(target=cancel_on_block,daemon=True).start()
  assert response(3)['stopReason']=='cancelled'
  proc.stdin.close();proc.wait(timeout=5)
  print('PASS debug-blocked-write-cancellation')
 finally:
  stop_tree(proc);server.shutdown();server.server_close()

if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--bin');run(find_binary(parser.parse_args().bin))
