"""Local usage ledger, informed by Token Ledger's MIT parsers.

Only allowlisted numeric usage and route metadata survive parsing. No prompts,
tool payloads, account files, environment dumps, or credentials are persisted.
Prices are downloaded only with --refresh-prices; reports work offline.
"""
from __future__ import annotations
import argparse, collections, datetime as dt, hashlib, json, math, os, pathlib, sqlite3, tempfile, urllib.request
from contextlib import closing

UTC=dt.timezone.utc
PRICE_URL='https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json'

def number(value):
    return max(0,int(value)) if isinstance(value,(int,float)) and not isinstance(value,bool) and math.isfinite(value) else 0

def dollars(value):
    return float(value) if isinstance(value,(int,float)) and not isinstance(value,bool) and math.isfinite(value) and value>=0 else None

def timestamp(value):
    try:
        if isinstance(value,(int,float)): return dt.datetime.fromtimestamp(value/1000 if value>1e12 else value,UTC).isoformat()
        date=dt.datetime.fromisoformat(str(value).replace('Z','+00:00'))
        return date.replace(tzinfo=UTC).isoformat() if date.tzinfo is None else date.astimezone(UTC).isoformat()
    except (ValueError,TypeError,OverflowError,OSError): return None

def metadata(value):
    return ''.join(char if char.isprintable() else ' ' for char in str(value or 'unknown'))[:160]

def event(agent,identity,date,model,provider,usage,cost=None):
    date=timestamp(date)
    if not date: return None
    counts={key:number(usage.get(key)) for key in ('input','output','cache_read','cache_write','reasoning')}
    if not sum(counts[k] for k in ('input','output','cache_read','cache_write')): return None
    return dict(id=hashlib.sha256((agent+'|'+str(identity)).encode()).hexdigest(),agent=agent,date=date,
                model=metadata(model),provider=metadata(provider),**counts,reported_usd=dollars(cost))

def json_lines(path):
    with path.open(encoding='utf-8',errors='replace') as source:
        for index,line in enumerate(source):
            # The logs can contain giant attachments. Never cache or emit a raw line.
            if len(line)>32*1024*1024: continue
            try: row=json.loads(line)
            except (ValueError,RecursionError): continue
            if isinstance(row,dict): yield index,row

def parse_jsonl(path,agent):
    events=[]; previous={}; model='unknown'; session=path.stem
    for index,row in json_lines(path):
        identity=f'{session}|{index}'; date=row.get('timestamp') or row.get('ts'); provider='unknown'; cost=None; estimate=None
        # Ignore malformed structural fields one row at a time, preserving later usage.
        if any(row.get(k) is not None and not isinstance(row[k],dict) for k in ('message','payload','usage')):continue
        for container in (row,row.get('message') or {},row.get('payload') or {}):
            if any(container.get(k) is not None and not isinstance(container[k],dict) for k in ('usage','info','cost','meta')):break
        else:container=None
        if container is not None:continue
        if agent=='Claude Code':
            message=row.get('message') or {}; u=message.get('usage') or {}
            if row.get('type')!='assistant' or not u or message.get('model')=='<synthetic>': continue
            model=message.get('model');provider='anthropic'
            identity=message.get('id') or identity
            usage=dict(input=u.get('input_tokens'),output=u.get('output_tokens'),cache_read=u.get('cache_read_input_tokens'),cache_write=u.get('cache_creation_input_tokens'))
        elif agent=='Codex':
            payload=row.get('payload') or {}
            if row.get('type')=='session_meta': session=payload.get('id') or session
            if row.get('type')=='turn_context': model=payload.get('model') or model;continue
            if payload.get('type')!='token_count': continue
            info=payload.get('info') or {};total=info.get('total_token_usage') or {}
            if not total: continue
            if any(number(total.get(k))<number(previous.get(k)) for k in ('input_tokens','output_tokens')):
                delta=info.get('last_token_usage') or {}
            else: delta={k:max(0,number(v)-number(previous.get(k))) for k,v in total.items()}
            previous=total
            provider='openai'; identity=f'{session}|{date}|{number(total.get("total_tokens"))}'
            cached=number(delta.get('cached_input_tokens'))
            usage=dict(input=max(0,number(delta.get('input_tokens'))-cached),output=delta.get('output_tokens'),cache_read=cached,cache_write=delta.get('cache_write_input_tokens'),reasoning=delta.get('reasoning_output_tokens'))
        elif agent=='Pi':
            message=row.get('message') or {};u=message.get('usage') or {}
            if message.get('role')!='assistant' or not u: continue
            model=message.get('model');provider=message.get('provider');date=message.get('timestamp') or date
            identity=message.get('responseId') or identity
            usage=dict(input=u.get('input'),output=u.get('output'),cache_read=u.get('cacheRead'),cache_write=u.get('cacheWrite'),reasoning=u.get('reasoning'))
            cost=(u.get('cost') or {}).get('total')
        elif agent=='Command Code':
            u=row.get('usage') or {}
            if not u: continue
            model=row.get('model');provider='commandcode'
            identity=((row.get('message') or {}).get('meta') or {}).get('messageId') or row.get('id') or identity
            cached=number(u.get('cacheReadTokens'))
            usage=dict(input=max(0,number(u.get('inputTokens'))-cached),output=u.get('outputTokens'),cache_read=cached,cache_write=u.get('cacheWriteTokens'))
            cost=u.get('costUsd')
        elif agent=='Nur':
            # Attempt lifecycle and aggregate rows are separate from the
            # per-request usage records; counting both would duplicate usage.
            if row.get('kind') in ('attempt','turn','session'): continue
            u=row.get('usage') or row
            model=row.get('model');provider=row.get('provider')
            cached=number(u.get('cached_tokens'))
            usage=dict(input=max(0,number(u.get('input_tokens'))-cached),output=u.get('output_tokens'),cache_read=cached,cache_write=u.get('cache_write_tokens'),reasoning=u.get('reasoning_tokens'))
            identity=row.get('response_id') or row.get('attempt_id') or row.get('request_id') or f"{row.get('session_id',session)}|{date}|{row.get('turn',index)}"
            provenance=u.get('cost_provenance') or row.get('cost_provenance')
            if provenance=='provider_reported': cost=u.get('cost_usd')
            if u.get('cost_known') and provenance in ('catalog_estimate','fallback_estimate'):
                estimate=dollars(u.get('cost_usd'))
        else: continue
        item=event(agent,identity,date,model,provider,usage,cost)
        if item:
            item['estimated_usd']=estimate
            events.append(item)
    return events

def parse_droid(path):
    row=json.loads(path.read_text(encoding='utf-8'));u=row.get('tokenUsage') or {}
    e=event('Droid',path.stem,path.stat().st_mtime,row.get('model'),'factory',dict(input=u.get('inputTokens'),output=u.get('outputTokens'),cache_read=u.get('cacheReadTokens'),cache_write=u.get('cacheCreationTokens'),reasoning=u.get('thinkingTokens')))
    return [e] if e else []

def parse_devin(path):
    events=[]
    # Read-only mode guarantees this cannot initialize or mutate a database.
    with closing(sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True)) as db:
        query="""SELECT json_extract(chat_message,'$.metadata') FROM message_nodes
                 WHERE json_extract(chat_message,'$.metadata.metrics') IS NOT NULL"""
        for (raw,) in db.execute(query):
            m=json.loads(raw);u=m.get('metrics') or {}
            e=event('Devin',m.get('request_id'),m.get('created_at'),m.get('generation_model'),'devin',dict(input=u.get('input_tokens'),output=u.get('output_tokens'),cache_read=u.get('cache_read_tokens'),cache_write=u.get('cache_creation_tokens')))
            if e: events.append(e)
    return events

def atomic_json(path,data):
    if path.parent.is_symlink():raise ValueError('refusing a symlink ledger directory')
    path.parent.mkdir(parents=True,exist_ok=True)
    if path.is_symlink(): raise ValueError('refusing a symlink ledger cache')
    fd,name=tempfile.mkstemp(prefix='.ledger-',dir=path.parent)
    try:
        with os.fdopen(fd,'w',encoding='utf-8') as stream: json.dump(data,stream,ensure_ascii=False,allow_nan=False)
        os.replace(name,path)
    finally:
        if os.path.exists(name): os.unlink(name)

def load_json(path,default):
    try: return json.loads(path.read_text(encoding='utf-8'))
    except (OSError,ValueError): return default

def discover(home,nur_home,isolated):
    codex=pathlib.Path(os.environ.get('CODEX_HOME',str(home/'.codex'))) if not isolated else home/'.codex'
    claude=pathlib.Path(os.environ.get('CLAUDE_CONFIG_DIR',str(home/'.claude'))) if not isolated else home/'.claude'
    roots=[('Claude Code',claude/'projects','*.jsonl'),('Claude Code',home/'.config/claude/projects','*.jsonl'),
           ('Codex',codex/'sessions','*.jsonl'),('Codex',codex/'archived_sessions','*.jsonl'),
           ('Droid',home/'.factory/sessions','*.settings.json'),('Pi',home/'.pi/agent/sessions','*.jsonl'),
           ('Command Code',home/'.commandcode/projects','*.jsonl'),('Nur',nur_home,'usage.jsonl')]
    for agent,root,pattern in roots:
        if root.exists():
            for path in root.rglob(pattern):
                if not path.is_symlink() and path.is_file(): yield agent,path
    path=home/'.local/share/devin/cli/sessions.db'
    if path.is_file() and not path.is_symlink(): yield 'Devin',path

def collect(home,nur_home,isolated=False):
    cache_path=nur_home/'ledger/usage-cache.json'
    cache=load_json(cache_path,{'version':1,'files':{}})
    if cache.get('version')!=1: cache={'version':1,'files':{}}
    warnings=collections.Counter(); present=collections.Counter()
    for agent,path in discover(home,nur_home,isolated):
        try:
            stat=path.stat();stamp=[stat.st_size,stat.st_mtime_ns]
            if agent=='Devin':
                wal=path.with_name(path.name+'-wal')
                if wal.exists(): stamp += [wal.stat().st_size,wal.stat().st_mtime_ns]
            identity=hashlib.sha256((agent+'|'+str(path.resolve())).encode()).hexdigest()
            present[agent]+=1
            if cache['files'].get(identity,{}).get('stamp')==stamp: continue
            events=parse_devin(path) if agent=='Devin' else parse_droid(path) if agent=='Droid' else parse_jsonl(path,agent)
            # For append-only logs keep numeric history after upstream rotation.
            cache['files'][identity]={'stamp':stamp,'events':events}
        except (OSError,ValueError,TypeError,AttributeError,sqlite3.Error,RecursionError): warnings[agent]+=1
    atomic_json(cache_path,cache)
    dedup={}
    for file in cache['files'].values():
        for e in file['events']:
            old=dedup.get(e['id'])
            # Streamed Claude updates repeat the message id; keep the most
            # complete usage object, never sum the repeated request.
            if old is None or sum(e[k] for k in ('input','output','cache_read','cache_write'))>=sum(old[k] for k in ('input','output','cache_read','cache_write')): dedup[e['id']]=e
    return list(dedup.values()),dict(warnings),dict(present)

def price(event,prices):
    if event.get('estimated_usd') is not None:return event['estimated_usd']
    model=event['model']; provider=event['provider']
    rates=prices.get(provider+'/'+model) or prices.get(model)
    if not isinstance(rates,dict) or 'input_cost_per_token' not in rates or 'output_cost_per_token' not in rates: return None
    components=[(event['input'],rates.get('input_cost_per_token')),(event['output'],rates.get('output_cost_per_token')),
                (event['cache_read'],rates.get('cache_read_input_token_cost')),(event['cache_write'],rates.get('cache_creation_input_token_cost'))]
    if any(count and dollars(rate) is None for count,rate in components): return None
    return sum(count*(dollars(rate) or 0) for count,rate in components)

def report(events,period,prices,now=None):
    now=now or dt.datetime.now().astimezone()
    today=now.replace(hour=0,minute=0,second=0,microsecond=0)
    start={'today':today,'7':today-dt.timedelta(days=6),'month':today.replace(day=1),'all':dt.datetime.min.replace(tzinfo=UTC)}[period]
    groups={};daily={}
    for e in events:
        date=dt.datetime.fromisoformat(e['date'])
        if date<start or date>now: continue
        key=(e['agent'],e['provider'],e['model'])
        g=groups.setdefault(key,dict(agent=key[0],provider=key[1],model=key[2],requests=0,input=0,output=0,cache_read=0,cache_write=0,reasoning=0,reported_usd=0.,estimated_usd=0.,unpriced_requests=0))
        g['requests']+=1
        for field in ('input','output','cache_read','cache_write','reasoning'): g[field]+=e[field]
        estimate=price(e,prices)
        if e.get('reported_usd') is not None: g['reported_usd']+=e['reported_usd']
        elif estimate is not None: g['estimated_usd']+=estimate
        else: g['unpriced_requests']+=1
        day=date.astimezone(now.tzinfo).date().isoformat()
        d=daily.setdefault(day,dict(day=day,tokens=0,reported_usd=0.,estimated_usd=0.))
        d['tokens']+=sum(e[k] for k in ('input','output','cache_read','cache_write'))
        if e.get('reported_usd') is not None: d['reported_usd']+=e['reported_usd']
        elif estimate is not None: d['estimated_usd']+=estimate
    rows=sorted(groups.values(),key=lambda r: r['input']+r['output']+r['cache_read']+r['cache_write'],reverse=True)
    return dict(period=period,groups=rows,daily=sorted(daily.values(),key=lambda r:r['day']),
                note='API price estimates are not subscription charges. Reasoning is included in output. Droid totals use the last-active date.')

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--period',choices=['today','7','month','all'],default='month')
    p.add_argument('--home',help='explicit log root (isolated fixtures or another home)')
    p.add_argument('--nur-home',required=True)
    p.add_argument('--refresh-prices',action='store_true')
    args=p.parse_args();nur_home=pathlib.Path(args.nur_home)
    price_path=nur_home/'ledger/prices.json'
    if args.refresh_prices:
        with urllib.request.urlopen(PRICE_URL,timeout=30) as response: raw=response.read(16*1024*1024)
        data=json.loads(raw)
        # Retain only prices, never unrelated upstream configuration.
        prices={model:{k:v for k,v in rates.items() if k in ('input_cost_per_token','output_cost_per_token','cache_read_input_token_cost','cache_creation_input_token_cost') and dollars(v) is not None} for model,rates in data.items() if isinstance(rates,dict)}
        atomic_json(price_path,prices)
        print(json.dumps({'models':len(prices),'source':PRICE_URL}));return
    events,warnings,sources=collect(pathlib.Path(args.home) if args.home else pathlib.Path.home(),nur_home,bool(args.home))
    result=report(events,args.period,load_json(price_path,{}));result['warnings']=warnings;result['sources']=sources
    print(json.dumps(result,ensure_ascii=False,allow_nan=False))

if __name__=='__main__': main()
