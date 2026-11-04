"""Refresh the website's shared integration credits and build-specific counts.

Run from nur-cli, optionally passing the sibling website checkout path.
The snapshot lets either site design build without access to this checkout.
"""
import json
from pathlib import Path
import re
import sys
from datetime import date

ROOT = Path(__file__).resolve().parents[1]
SITE = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT.parent / 'nuroctane.xyz'
catalog = (ROOT/'src/plugins/catalog.rs').read_text(encoding='utf-8')
credits = []
for block in re.findall(r'PluginEntry\s*\{(.*?)\n\s*\}', catalog, re.S):
    fields = dict(re.findall(r'(name|description|source_url):\s*"([^"\n]*)"', block))
    if not all(key in fields for key in ('name', 'description', 'source_url')):
        continue
    credits.append({'name': fields['name'], 'href': fields['source_url'].removesuffix('.git'),
                    'why': fields['description'].replace('\u2014', '-')})
if not credits:
    raise SystemExit('No plugin credits parsed; refusing to replace the snapshot')
known = {credit['href'].lower() for credit in credits}
upstream = json.loads((ROOT/'skills/upstream-lock.json').read_text(encoding='utf-8'))
for repository in sorted({entry['repository'].removesuffix('.git') for entry in upstream['skills'].values()}):
    if repository.lower() in known:
        continue
    owner, name = repository.rstrip('/').split('/')[-2:]
    credits.append({'name': f'{owner} / {name}', 'href': repository,
                    'why': f'Pinned upstream skill source by {owner}; mirrored with commit and file hashes'})
    known.add(repository.lower())
market = json.loads((ROOT/'skills/market-sources.json').read_text(encoding='utf-8'))
credits.append({'name': 'aidentity', 'href': 'https://github.com/'+market['aidentity']['repo'],
                'why': 'CC0 discovery metadata for the signed skills index; skill files come from author repositories'})
destination = SITE/'shared/content/nur-plugin-credits.json'
destination.write_text(json.dumps(credits, indent=2, ensure_ascii=False)+'\n', encoding='utf-8')
print(f'{len(credits)} integration credits -> {destination}')

tools = (ROOT/'src/tools/mod.rs').read_text(encoding='utf-8').split('fn boxed_tools')[1].split('pub fn tool_defs')[0]
lean = (ROOT/'src/tools/tool_search.rs').read_text(encoding='utf-8').split('pub const LEAN_ALWAYS_ON:')[1].split('];')[0]
providers = (ROOT/'src/providers.rs').read_text(encoding='utf-8').split('pub const PROVIDERS:')[1].split('\n];')[0]
version = re.search(r'^version = "([^"]+)"', (ROOT/'Cargo.toml').read_text(), re.M)[1]
for design in ('blackboard', 'digital-sea'):
    destination = SITE/f'artifacts/{design}/src/data/nurCounts.json'
    if not destination.exists():
        continue
    counts = json.loads(destination.read_text())
    counts.update(date=date.today().isoformat(), version=version, routes=len(re.findall(r'\bid:\s*"', providers)),
                  tools=tools.count('Box::new('), leanTools=len(re.findall(r'^\s*"', lean, re.M))+1)
    destination.write_text(json.dumps(counts, indent=2)+'\n', encoding='utf-8')
    print(f'{design}: {counts}')

    architecture = SITE/f'artifacts/{design}/src/data/nurArchitecture.json'
    if architecture.exists():
        data = json.loads(architecture.read_text(encoding='utf-8'))
        data['project'].update(date=counts['date'], tagline=
            f"{counts['routes']} provider routes, {counts['tools']} tools and {counts['jevEngines']} Jev engines in one Rust agent")
        for node in data['graph']['nodes']:
            if node['label'] == 'Permission + ToolHost':
                node['sub'] = f"{counts['tools']} tools / one dispatcher"
            # Paths survive edits; stale line numbers do not.
            node['sourceRef'] = re.sub(r':\d+$', '', node.get('sourceRef', ''))
        additions = [
            ('secrets', 'Secret store', 'store', 'masked input / redacted output', 'src/secrets.rs',
             'Credentials enter through a masked modal, stay in an owner-only local store, reach shell commands as environment variables, and are redacted from tool results.'),
            ('auto_continue', 'Continuation scheduler', 'service', 'usage reset / outage recovery', 'src/agent/resume.rs',
             'Usage limits and transient outages schedule a continuation with a visible countdown and user controls. Billing and sign-in failures do not automatically continue.'),
            ('skills_market', 'Verified skills', 'service', 'signed index / pinned authors', 'src/skills_market.rs',
             'The user installs from a signed index. Author files are pinned by commit and SHA-256 and scanned for hidden instructions; the model can search without installing.'),
            ('code_security', 'Code and security scans', 'tool', 'slop_scan / secscan', 'src/tools/secscan.rs',
             'Structural checks report code-shape findings. Optional Gitleaks, OSV-Scanner, Semgrep, Trivy, Checkov and TruffleHog normalize security findings; marking a finding as noise always asks the user.'),
        ]
        existing = {node['id'] for node in data['graph']['nodes']}
        for node_id, label, kind, sub, source, detail in additions:
            if node_id not in existing:
                data['graph']['nodes'].append(dict(id=node_id, label=label, kind=kind,
                    sub=sub, sourceRef=source, detail=detail))
            elif node_id == 'code_security':
                next(node for node in data['graph']['nodes'] if node['id'] == node_id)['detail'] = detail
        for source, target, label in [('tool_runtime', 'secrets', 'inject / redact'),
                ('agent_loop', 'auto_continue', 'schedule / resume'),
                ('runtime_entry', 'skills_market', 'user installs'),
                ('tool_runtime', 'code_security', 'inspect findings')]:
            if not any(edge['from'] == source and edge['to'] == target for edge in data['graph']['edges']):
                data['graph']['edges'].append({'from': source, 'to': target, 'kind': 'calls', 'label': label})
        architecture.write_text(json.dumps(data, indent=2, ensure_ascii=False)+'\n', encoding='utf-8')
