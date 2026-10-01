"""Wire failures: key leakage, redirects, wrong labels, and protocol translation."""
import importlib.util, json, pathlib, sys, threading, unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'scripts'))
from jev_engine_gateway import HttpAdapter, validate_endpoint, make_task

class GatewayTests(unittest.TestCase):
    def test_credentials_require_tls_and_clean_url(self):
        for url in ['http://example.com/v1', 'https://user:secret@example.com', 'file:///tmp/x', 'http://host.docker.internal']:
            with self.assertRaises(ValueError): validate_endpoint(url)
        self.assertEqual(validate_endpoint('http://127.0.0.1:8999'), 'http://127.0.0.1:8999')

    def test_translate_list_and_reject_redirect(self):
        seen=[]
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args): pass
            def do_POST(self):
                body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                seen.append((self.path,body,self.headers.get('Authorization')))
                if self.path=='/redirect/v1/systemone':
                    self.send_response(307); self.send_header('Location','/sink'); self.end_headers(); return
                self.send_response(200); self.end_headers()
                self.wfile.write(json.dumps({'answers':[{'type':'noul','noul':.8}]}).encode())
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            url=f'http://127.0.0.1:{server.server_port}'
            task=make_task('noul',{'type':'noul','instructions':'Does this hold?'},'state')
            result=HttpAdapter(url,'model','systemone-list').run(task)
            self.assertAlmostEqual(result.probs['no'],.2)
            self.assertAlmostEqual(result.probs['yes'],.8)
            self.assertIsNone(seen[0][2])
            self.assertEqual(seen[0][1]['questions'],[task.question])
            with self.assertRaises(ValueError): HttpAdapter(url+'/redirect','model','systemone').run(task)
            self.assertEqual(len(seen),2)
        finally: server.shutdown();server.server_close()

    def test_unknown_labels_are_no_judgment(self):
        from jev_engine_gateway import validate_probs
        for probabilities in [{'a':.8,'invented':.2},{'a':float('nan'),'b':.5},{'a':0,'b':0},{'a':2,'b':-1}]:
            with self.assertRaises(ValueError): validate_probs(probabilities,['a','b'])

    def test_receipt_cannot_move_credentials_to_another_origin(self):
        import os
        from unittest.mock import patch
        seen=[]
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_POST(self):
                seen.append(self.headers.get('Authorization'))
                self.send_response(202);self.end_headers()
                self.wfile.write(json.dumps({'status_url':'https://example.com/steal'}).encode())
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            with patch.dict(os.environ,{'NUR_TEST_JEV_KEY':'fixture-engine-key'}):
                adapter=HttpAdapter(f'http://127.0.0.1:{server.server_port}','djev','djev','NUR_TEST_JEV_KEY')
                with self.assertRaisesRegex(ValueError,'different origin'):adapter.run(make_task('noul',{'type':'noul','instructions':'q'},'state'))
            self.assertEqual(seen,['Bearer fixture-engine-key'])
        finally:server.shutdown();server.server_close()

    def test_choice_source_rows_and_label_only_abstention(self):
        from jev_local_bridge import answer_for,ContractError
        from unittest.mock import patch
        task=make_task('choice',{'type':'choice','instructions':'q','criteria':{'a':'A','b':'B'}},'state')
        adapter=HttpAdapter('http://localhost:1','m','choice')
        with patch.object(adapter,'post',return_value={'options':[{'index':1,'option':'a: A','probability':.8},{'index':2,'option':'b: B','probability':.2}]}):
            self.assertEqual(adapter.run(task).probs,{'a':.8,'b':.2})
        answer=answer_for('choice',task.question,{'__nur_label_only__':'a'})
        self.assertEqual(answer['choice'],'a');self.assertEqual(answer['confidence'],0);self.assertEqual(answer['probabilities'],{})
        with self.assertRaises(ContractError):answer_for('noul',{'type':'noul'}, {'__nur_label_only__':'yes'})

    def test_secret_values_are_not_adapter_options(self):
        from jev_engine_gateway import adapter_options
        for raw in ['{"api_key":"private"}','{"request_options":{"Authorization":"Bearer private"}}']:
            with self.assertRaises(ValueError):adapter_options(raw)
        self.assertEqual(adapter_options('{"max_tokens":20}'),{'max_tokens':20})

    def test_djev_session_token_receipt_polling_and_reuse(self):
        import os,time
        from unittest.mock import patch
        seen=[];polls=0
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def reply(self,status,body):
                self.send_response(status);self.end_headers();self.wfile.write(json.dumps(body).encode())
            def do_GET(self):
                nonlocal polls
                seen.append((self.path,self.headers.get('Authorization')))
                if self.path=='/v1/access/session':
                    self.reply(200,{'access_token':'fixture-short-lived','expires_at':time.time()+300})
                else:
                    polls+=1
                    self.reply(200,{'status':'running','retry_after':.1} if polls%2 else {'status':'succeeded','result':{'answers':{'decision':{'noul':.9}}}})
            def do_POST(self):
                self.rfile.read(int(self.headers['Content-Length']))
                seen.append((self.path,self.headers.get('Authorization')))
                self.reply(202,{'status_url':'/receipt/one','retry_after':.1})
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            with patch.dict(os.environ,{'NUR_TEST_SESSION':'fixture-long-lived','DJEV_API_KEY':'fixture-unselected'}):
                adapter=HttpAdapter(f'http://127.0.0.1:{server.server_port}','djev','djev',options={'session_env':'NUR_TEST_SESSION'})
                task=make_task('noul',{'type':'noul','instructions':'q'},'state')
                self.assertEqual(adapter.run(task).probs['yes'],.9)
                self.assertEqual(adapter.run(task).probs['yes'],.9)
            self.assertEqual(sum(path=='/v1/access/session' for path,_ in seen),1)
            self.assertEqual(seen[0][1],'Bearer fixture-long-lived')
            self.assertTrue(all(auth=='Bearer fixture-short-lived' for _,auth in seen[1:]))
            self.assertEqual(polls,4)
        finally:server.shutdown();server.server_close()

    def test_classifier_score_texts_and_unscored_choice_are_not_fabricated(self):
        from unittest.mock import patch
        from jev_local_bridge import AdapterBackend,ContractError
        adapter=HttpAdapter('http://localhost:1','fast','classifier-dev')
        task=make_task('score',{'type':'score','instructions':'severity','criteria':['minor','critical']},'state')
        with patch.object(adapter,'post',return_value={'results':[{'scores':{'minor':.1,'critical':.9}}]}) as request:
            self.assertEqual(adapter.run(task).probs,{'0':.1,'1':.9})
            self.assertEqual(request.call_args.args[1]['labels'],['minor','critical'])
        backend=AdapterBackend();backend._adapter=adapter
        question={'type':'choice','instructions':'q','criteria':{'a':'A','b':'B'}}
        with patch.object(adapter,'post',return_value={'results':[{'label':'a','confidence':.99,'unscored':'input'}]}):
            self.assertEqual(backend.decide('choice',question,'state'),{'__nur_label_only__':'a'})
        with patch.object(adapter,'post',return_value={'results':[{'label':'yes','confidence':.99}]}):
            with self.assertRaises(ContractError):backend.decide('noul',{'type':'noul','instructions':'q'},'state')

if __name__=='__main__': unittest.main()
