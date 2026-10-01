"""Local log failure modes: duplicates, counters, missing prices and secret retention."""
import datetime as dt, json, pathlib, sqlite3, sys, tempfile, unittest
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'scripts'))
import usage_ledger as ledger
from contextlib import closing

class LedgerTests(unittest.TestCase):
    def test_claude_stream_duplicates_and_cache_excludes_secrets(self):
        with tempfile.TemporaryDirectory() as folder:
            home=pathlib.Path(folder);nur=home/'.nur'
            path=home/'.claude/projects/test/a.jsonl';path.parent.mkdir(parents=True)
            row={'type':'assistant','timestamp':'2026-10-01T12:00:00Z','message':{'id':'request1','model':'claude-test','usage':{'input_tokens':10,'output_tokens':2,'cache_read_input_tokens':20},'content':'TOP_SECRET_API_KEY'}}
            path.write_text(json.dumps(row)+'\n'+json.dumps(row)+'\n'+ '{broken\n',encoding='utf-8')
            events,warnings,sources=ledger.collect(home,nur,True)
            self.assertEqual(len(events),1);self.assertEqual(events[0]['cache_read'],20)
            self.assertNotIn('TOP_SECRET', (nur/'ledger/usage-cache.json').read_text())
            path.unlink()
            self.assertEqual(ledger.collect(home,nur,True)[0],events)

    def test_codex_deltas_resets_and_repeated_totals(self):
        with tempfile.TemporaryDirectory() as folder:
            path=pathlib.Path(folder)/'session.jsonl'
            def count(i,o,last=None):
                return {'timestamp':'2026-10-01T12:00:00Z','payload':{'type':'token_count','info':{'total_token_usage':{'input_tokens':i,'output_tokens':o,'total_tokens':i+o},'last_token_usage':last}}}
            rows=[{'type':'turn_context','payload':{'model':'codex-test'}},count(100,10),count(100,10),count(120,15),count(5,1,{'input_tokens':5,'output_tokens':1})]
            path.write_text('\n'.join(json.dumps(r) for r in rows),encoding='utf-8')
            events=ledger.parse_jsonl(path,'Codex')
            self.assertEqual(sum(e['input'] for e in events),125)
            self.assertEqual(sum(e['output'] for e in events),16)
            self.assertEqual(len(events),3)

    def test_unpriced_is_not_zero_cost_and_reasoning_is_not_double_counted(self):
        e=ledger.event('Pi','id','2026-10-01T12:00:00Z','new-model','vendor',dict(input=100,output=20,reasoning=15))
        report=ledger.report([e],'all',{},now=dt.datetime(2026,10,2,tzinfo=ledger.UTC))
        self.assertEqual(report['groups'][0]['unpriced_requests'],1)
        self.assertEqual(report['daily'][0]['tokens'],120)
        e['reported_usd']=0
        self.assertEqual(ledger.report([e],'all',{},now=dt.datetime(2026,10,2,tzinfo=ledger.UTC))['groups'][0]['unpriced_requests'],0)

    def test_nur_real_log_shape_and_devin_read_only(self):
        with tempfile.TemporaryDirectory() as folder:
            path=pathlib.Path(folder)/'usage.jsonl'
            path.write_text(json.dumps({'ts':'2026-10-01T12:00:00Z','session_id':'s','model':'m','provider':'openai','usage':{'input_tokens':100,'output_tokens':20,'cached_tokens':30,'cost_provenance':'provider_reported','cost_usd':.5}}))
            result=ledger.parse_jsonl(path,'Nur')[0]
            self.assertEqual(result['input'],70);self.assertEqual(result['reported_usd'],.5)
            db=path.with_suffix('.db')
            with closing(sqlite3.connect(db)) as connection:
                connection.execute('CREATE TABLE message_nodes(chat_message TEXT)')
                connection.execute('INSERT INTO message_nodes VALUES (?)',(json.dumps({'metadata':{'request_id':'x','created_at':'2026-10-01T12:00:00Z','generation_model':'m','metrics':{'input_tokens':15}}}),))
                connection.commit()
            before=db.read_bytes();self.assertEqual(ledger.parse_devin(db)[0]['input'],15);self.assertEqual(before,db.read_bytes())

    def test_pi_commandcode_droid_cache_and_reported_cost(self):
        with tempfile.TemporaryDirectory() as folder:
            path=pathlib.Path(folder)/'usage.jsonl'
            path.write_text(json.dumps({'type':'message','message':{'role':'assistant','timestamp':1790856000000,'model':'pi-test','provider':'vendor','usage':{'input':12,'output':4,'cacheRead':8,'cacheWrite':3,'cost':{'total':.25}}}}))
            pi=ledger.parse_jsonl(path,'Pi')[0]
            self.assertEqual((pi['input'],pi['cache_read'],pi['cache_write'],pi['reported_usd']),(12,8,3,.25))
            path.write_text(json.dumps({'timestamp':'2026-10-01T12:00:00Z','model':'cc-test','usage':{'inputTokens':30,'outputTokens':5,'cacheReadTokens':20,'costUsd':.1}}))
            command=ledger.parse_jsonl(path,'Command Code')[0]
            self.assertEqual((command['input'],command['cache_read'],command['reported_usd']),(10,20,.1))
            path.write_text(json.dumps({'model':'droid-test','tokenUsage':{'inputTokens':11,'outputTokens':2,'cacheReadTokens':9,'cacheCreationTokens':4,'thinkingTokens':1}}))
            droid=ledger.parse_droid(path)[0]
            self.assertEqual((droid['input'],droid['cache_read'],droid['cache_write']),(11,9,4))
            report=ledger.report([pi,command,droid],'all',{},now=dt.datetime.now(ledger.UTC)+dt.timedelta(days=1))
            self.assertAlmostEqual(sum(g['reported_usd'] for g in report['groups']),.35)
            self.assertEqual(sum(g['unpriced_requests'] for g in report['groups']),1)

    def test_nur_estimate_is_not_a_provider_bill_and_bad_rows_do_not_hide_usage(self):
        with tempfile.TemporaryDirectory() as folder:
            path=pathlib.Path(folder)/'usage.jsonl'
            rows=[{'message':[]},{'usage':{'input_tokens':20,'output_tokens':4,'cost_provenance':'catalog_estimate','cost_known':True,'cost_usd':.5},'ts':'2026-10-01T12:00:00Z','model':'m','provider':'vendor'}]
            path.write_text('\n'.join(json.dumps(row) for row in rows))
            report=ledger.report(ledger.parse_jsonl(path,'Nur'),'all',{},now=dt.datetime(2026,10,2,tzinfo=ledger.UTC))
            self.assertEqual(report['groups'][0]['reported_usd'],0)
            self.assertEqual(report['groups'][0]['estimated_usd'],.5)

if __name__=='__main__': unittest.main()
