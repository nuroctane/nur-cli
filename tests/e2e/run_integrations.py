"""Black-box Jev catalog, bridge lifecycle, key isolation and usage ledger checks."""
import argparse,json,os,pathlib,socket,subprocess,tempfile,threading,urllib.request
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin',default='target/debug/nur.exe');args=parser.parse_args()
    binary=str(pathlib.Path(args.bin).resolve());seen=[]
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            seen.append({'path':self.path,'body':body,'auth':self.headers.get('Authorization')})
            self.send_response(200);self.end_headers()
            self.wfile.write(json.dumps({'model':'fixture','answers':[{'type':'noul','noul':.8}]}).encode())
    upstream=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=upstream.serve_forever,daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='nur-integrations-') as folder:
        home=pathlib.Path(folder);nurhome=home/'.nur';nurhome.mkdir()
        env=dict(os.environ,NUR_HOME=str(nurhome),TYPESAFE_API_KEY='fixture-typesafe-must-not-leak',NUR_TEST_ENGINE_KEY='fixture-selected-key')
        for key in ('NUR_JEV_LOCAL_URL','CODEX_HOME','CLAUDE_CONFIG_DIR'):env.pop(key,None)
        def run(*arguments,success=True):
            result=subprocess.run([binary,*arguments],env=env,capture_output=True,text=True,encoding='utf8',errors='replace',timeout=90)
            if success and result.returncode:raise AssertionError(result.stderr or result.stdout)
            return result
        try:
            rows=json.loads(run('jev','models','--json').stdout)
            assert len(rows)==112 and len({row['id'] for row in rows})==112
            assert 'Mica' in run('jev','info','mica-v01-4b').stdout
            before=(nurhome/'config.toml').read_bytes()
            assert run('jev','use','--engine','instinct','--url','https://user:secret@example.com',success=False).returncode!=0
            assert (nurhome/'config.toml').read_bytes()==before
            run('jev','use','--engine','instinct')
            text=(nurhome/'config.toml').read_text(encoding='utf8')
            assert 'jev:instinct' in text and 'fixture-typesafe' not in text
            # Switching away from an old inline key preserves it in the vault,
            # clears the config, and explicitly enables the selected local route.
            import tomllib
            text=text.replace('[typesafe]\n','[typesafe]\napi_key = "fixture-legacy-key"\n',1) if 'api_key =' not in text else text.replace('api_key = ""','api_key = "fixture-legacy-key"',1)
            text=text.replace('enabled = true','enabled = false')
            (nurhome/'config.toml').write_text(text,encoding='utf8')
            with socket.socket() as free:
                free.bind(('127.0.0.1',0));port=free.getsockname()[1]
            run('jev','start','--engine','system-one-open','--upstream',f'http://127.0.0.1:{upstream.server_port}','--key-env','NUR_TEST_ENGINE_KEY','--port',str(port))
            run('jev','use','--port',str(port))
            selected_config=tomllib.loads((nurhome/'config.toml').read_text())['typesafe']
            assert selected_config['enabled'] and not selected_config.get('api_key') and not selected_config.get('credential_provider')
            request={'state':'local fixture','questions':{'decision':{'type':'noul','instructions':'Does this hold?'}}}
            with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/v1/systemone',json.dumps(request).encode(),{'Content-Type':'application/json'}),timeout=10) as response:answer=json.load(response)
            assert answer['answers']['decision']['noul']==.8
            assert seen[-1]['auth']=='Bearer fixture-selected-key'
            assert isinstance(seen[-1]['body']['questions'],list)
            assert 'fixture-selected-key' not in (nurhome/'jev/bridge.json').read_text()
            assert 'fixture-selected-key' not in (nurhome/'config.toml').read_text()
            run('jev','stop')
            run('jev','use','--url',f'http://127.0.0.1:{port}','--key-env','NUR_MISSING_EXPLICIT_KEY')
            assert 'selected environment slot unavailable' in run('jev','status').stdout
            log=home/'.claude/projects/fixture.jsonl';log.parent.mkdir(parents=True)
            log.write_text(json.dumps({'type':'assistant','timestamp':'2026-09-30T12:00:00Z','message':{'id':'one','model':'fixture-model','usage':{'input_tokens':20,'output_tokens':5},'content':[{'text':'fixture-super-secret'}]}})+'\n',encoding='utf8')
            report=json.loads(run('ledger','--home',str(home),'--period','all','--json').stdout)
            assert report['groups'][0]['input']==20 and report['groups'][0]['output']==5
            assert report['groups'][0]['unpriced_requests']==1
            assert 'fixture-super-secret' not in (nurhome/'ledger/usage-cache.json').read_text()
            assert 'fixture-selected-key' not in json.dumps(report)
            print('PASS catalog: all 112 unique systems; native selection; URL rejection')
            print('PASS real bridge: list protocol translation; selected key only; clean persisted state; stop')
            print('PASS real ledger: no chat login; local tokens; unpriced distinction; secret exclusion')
        finally:
            run('jev','stop',success=False)
            upstream.shutdown();upstream.server_close()

if __name__=='__main__':main()
