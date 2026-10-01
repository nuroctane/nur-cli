"""Vendor the reviewed MIT local adapters from a pinned JevBench checkout."""
import argparse,hashlib,json,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[1]
FILES=('base','gradio_space','local_openjev','semif_direct','so1_decider','sg_system_one',
       'laya_local','gliner2_local','verdict_local','paw_local','certo_local','smalljev_local','needle_local')
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--checkout',required=True);p.add_argument('--revision',required=True);a=p.parse_args()
    source=pathlib.Path(a.checkout)
    sha=subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()
    if sha!=a.revision: raise SystemExit('checkout differs from the reviewed revision')
    dest=ROOT/'scripts/jev_adapters';dest.mkdir(exist_ok=True)
    hashes={}
    for name in FILES:
        raw=(source/'jevbench/adapters'/f'{name}.py').read_text(encoding='utf-8').replace('\r\n','\n')
        (dest/f'{name}.py').write_text(raw,encoding='utf-8')
        hashes[f'{name}.py']=hashlib.sha256(raw.encode()).hexdigest()
    (dest/'__init__.py').write_text('"""Pinned JevBench local adapters. Heavy dependencies load only on demand."""\n',encoding='utf-8')
    (dest/'LICENSE').write_bytes((source/'LICENSE').read_bytes())
    (dest/'UPSTREAM.json').write_text(json.dumps(dict(repository='https://github.com/fstandhartinger/jevbench',commit=sha,files=hashes),indent=2)+'\n',encoding='utf-8')
    print(f'vendored {len(FILES)} MIT adapter modules from {sha}')
if __name__=='__main__':main()
