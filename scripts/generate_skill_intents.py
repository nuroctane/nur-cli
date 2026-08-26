#!/usr/bin/env python3
"""
Generate src/agent/skill_intents.json — comprehensive 700+ skill intent index.
Must be run when adding new skills per AGENTS.md.

Scans ~/.nur/skills + ~/.agents/skills + repo skills/ and generates triggers:
- name, /name, spaced, /spaced
- bigrams/trigrams from name
- aliases for known shorthands
- description bigram

Usage:
  python scripts/generate_skill_intents.py
  # or from repo root: python -m scripts.generate_skill_intents
"""
import pathlib, json, re

skills_root = pathlib.Path.home() / ".nur" / "skills"
agents_skills_root = pathlib.Path.home() / ".agents" / "skills"
repo_root = pathlib.Path(__file__).parent.parent
output = repo_root / "src" / "agent" / "skill_intents.json"

STOP = set(["a","an","the","and","or","to","of","for","in","on","at","by","with","from","this","that","these","those","is","are","was","were","be","been","being","have","has","had","do","does","did","will","would","can","could","should","may","might","must","use","using","used","when","where","what","which","who","how","why","into","over","under","about","after","before","your","you","their","them","its","it","as","if","then","than","also","just","only","not","no","yes","any","all","each","other","more","most","some","such","via","per","between","through","during","without","within","skill","skills","agent","agents","help","please","like","make","need","needs","want","wants","get","set","run","work","works","working"])

def parse_frontmatter_scalar(fm, key):
    """Plain, quoted, or YAML folded (`>-` / `|`) scalars. Bare `>-` is missing."""
    lines = fm.splitlines()
    prefix = f"{key}:"
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.startswith(prefix):
            i += 1
            continue
        rest = line[len(prefix):].strip()
        if rest in (">", ">-", ">+", "|", "|-", "|+"):
            i += 1
            parts = []
            while i < len(lines):
                l = lines[i]
                if l.startswith(" ") or l.startswith("\t"):
                    t = l.strip()
                    if t:
                        parts.append(t)
                    i += 1
                elif not l.strip():
                    i += 1
                else:
                    break
            joined = " ".join(parts)
            return joined or ""
        s = rest.strip().strip('"').strip("'").strip()
        if s in (">-", "|", ">", "|-"):
            return ""
        return s
    return ""

def gen_triggers(name, desc):
    triggers = set()
    triggers.add(name)
    triggers.add(f"/{name}")
    spaced = name.replace("-", " ")
    triggers.add(spaced)
    triggers.add(f"/{spaced}")
    parts = [p for p in name.split("-") if len(p) >= 2]
    for i in range(len(parts)):
        for j in range(i+1, min(i+3, len(parts))):
            phrase = " ".join(parts[i:j+1])
            if len(phrase) >= 7:
                triggers.add(phrase)
                triggers.add(phrase.replace(" ", "-"))
                triggers.add(f"/{phrase}")
                triggers.add(f"/{phrase.replace(' ', '-')}")
    if "-" not in name and len(name) >= 3:
        triggers.add(name)
        triggers.add(f"/{name}")
    ALIASES = {
        "test-driven-development": ["tdd", "tdd this", "test driven"],
        "systematic-debugging": ["debug systematically"],
        "fable-method": ["think like fable", "fable method", "the fable method"],
        "fable-loop": ["fable loop", "run the fable loop"],
        "fable-judge": ["fable judge", "fable judge this"],
        "scan": ["codebase scan"],
        "interior": [
            "interior.dev",
            "micro interaction",
            "micro-interactions",
            "half second",
            "half-second",
        ],
        "sc-research": [
            "whitehat crypto",
            "smart contract researcher",
            "web3 security research",
            "defi whitehat",
            "sc research",
        ],
        "contest-and-bounty-reporting": [
            "immunefi report",
            "code4rena finding",
            "cantina finding",
            "sherlock contest",
        ],
        "onchain-read-recon": [
            "cast storage",
            "sourcify recon",
            "onchain recon",
        ],
        "reviewing-oracles-and-pricing": [
            "oracle manipulation",
            "flash loan price",
            "chainlink staleness",
        ],
        "writing-foundry-invariant-handlers": [
            "invariant handler",
            "ghost variables",
            "foundry invariant",
        ],
        "reviewing-erc4626-and-vaults": [
            "erc4626",
            "vault inflation",
            "first depositor",
            "belt finance bounty",
        ],
        "reviewing-amm-and-cl-pools": [
            "uniswap callback",
            "concentrated liquidity",
            "amm invariant",
            "balancer rounding",
        ],
        "reviewing-lending-and-liquidations": [
            "liquidation bonus",
            "lending market",
            "bad debt",
            "notional free collateral",
        ],
        "reviewing-bridges-and-messaging": [
            "bridge replay",
            "layerzero",
            "cross chain message",
            "scroll message spoof",
        ],
        "reviewing-upgradeable-proxies": [
            "uninitialized proxy",
            "wormhole uninitialized",
            "uups initialize",
        ],
        "reviewing-l2-sequencer-and-finality": [
            "l2 sequencer",
            "optimism selfdestruct",
            "zksync proof",
        ],
        "reviewing-access-control-and-auth": [
            "missing onlyowner",
            "enzyme privilege",
        ],
        "reviewing-governance-and-timelocks": [
            "governor timelock",
            "flashloan vote",
        ],
        "reviewing-token-standard-pitfalls": [
            "fee on transfer",
            "weird erc20",
        ],
        "reviewing-signatures-permit-and-eip712": [
            "eip-712",
            "permit replay",
            "ecrecover",
        ],
        "reviewing-reentrancy-and-callbacks": [
            "read-only reentrancy",
            "erc777 hook",
        ],
        "formal-verification-halmos-certora-kontrol": [
            "halmos",
            "certora cvl",
            "kontrol",
        ],
        "researcher-gym-and-curriculum": [
            "ethernaut",
            "damn vulnerable defi",
            "cyfrin updraft",
        ],
        "reconstructing-public-postmortems": [
            "defihacklabs",
            "rekt writeup",
        ],
        "battlechain-safe-harbor-whitehat": [
            "battlechain",
            "safe harbor",
        ],
        "auditing-blockchain-clients": [
            "client auditor",
            "firedancer",
        ],
        "historical-smart-contract-vulns": [
            "immunefi bounty library",
            "defi hack history",
            "historical vulns",
            "wormhole bounty class",
            "zachxbt",
            "tayvano",
            "investigator ops",
        ],
        "hunting-x-linked-bounties": [
            "x bounty intel",
            "immunefi payout",
            "paid immunefi",
            "whitehat bounty writeup",
            "satya0x",
            "pwning.eth",
        ],
        "reviewing-solana-programs": [
            "solana program review",
            "anchor audit",
            "missing signer",
            "raydium tick",
        ],
        "reviewing-move-modules": [
            "sui move review",
            "aptos module",
            "shared object acl",
            "sui network shutdown",
        ],
        "reviewing-cosmos-and-ibc": [
            "ibc ics-20",
            "cosmwasm reentrancy",
            "dragonberry",
            "sei bounty",
        ],
        "reviewing-cairo-and-starknet": [
            "cairo rounding",
            "starknet vesu",
            "vesu rounding",
        ],
        "reviewing-bitcoin-adjacent": [
            "bitcoin script review",
            "lightning htlc",
            "stacks clarity",
            "stacks dos bounty",
        ],
        "reviewing-frontend-and-ops-surfaces": [
            "bybit safe ui",
            "permit2 phishing",
            "wallet drainer",
        ],
    }
    alias_kept = []
    if name in ALIASES:
        for a in ALIASES[name]:
            triggers.add(a)
            alias_kept.append(a)
    tokens = [w.lower() for w in re.findall(r"[A-Za-z0-9]+", desc.lower()) if len(w)>=4 and w not in STOP]
    if len(tokens) >= 2:
        bigram = f"{tokens[0]} {tokens[1]}"
        if len(bigram) >= 10:
            triggers.add(bigram)
    filtered = []
    for t in triggers:
        if len(t) < 5:
            continue
        if " " not in t and "-" not in t and "/" not in t:
            if t != name and len(t) < 7:
                continue
        if t.lower() in STOP:
            continue
        if t.lower().strip("/ ") == "fable":
            continue
        if t.lower() in ["/fable", "fable"]:
            continue
        filtered.append(t)
    filtered = sorted(set(filtered), key=lambda x: (-len(x), x))[:12]
    for a in alias_kept:
        if a not in filtered:
            filtered.append(a)
    return filtered

skills = []
for src in [skills_root, agents_skills_root, repo_root / "skills"]:
    if not src.exists():
        continue
    for md in src.rglob("SKILL.md"):
        if "references" in md.parts:
            continue
        try:
            text = md.read_text(encoding="utf-8", errors="ignore")
        except:
            continue
        folder = md.parent.name
        name = folder
        desc = ""
        if text.startswith("---"):
            end = text.find("---", 3)
            if end != -1:
                fm = text[3:end]
                n = parse_frontmatter_scalar(fm, "name")
                if n:
                    name = n
                desc = parse_frontmatter_scalar(fm, "description")
                if not desc:
                    body = text[end+3:].strip()
                    for line in body.splitlines():
                        if line.strip():
                            desc = line.strip().lstrip("#").strip()[:200]
                            break
        else:
            for line in text.splitlines():
                if line.strip():
                    desc = line.strip().lstrip("#").strip()[:200]
                    break
        if any(s["name"] == name for s in skills):
            continue
        triggers = gen_triggers(name, desc)
        tokens = [w.lower() for w in re.findall(r"[A-Za-z0-9]+", (name + " " + desc).lower()) if len(w)>=4 and w not in STOP]
        keywords = list(dict.fromkeys(tokens))[:10]
        skills.append({
            "name": name,
            "description": desc[:300],
            "path": str(md),
            "triggers": triggers,
            "keywords": keywords
        })

skills_sorted = sorted(skills, key=lambda x: x["name"])
out_data = {
    "generated_at": __import__("datetime").datetime.utcnow().isoformat(),
    "total_skills": len(skills_sorted),
    "note": "Comprehensive skill intent index - 700+ skills, expanded NL triggers. Must be updated when adding skills per AGENTS.md. Run: python scripts/generate_skill_intents.py",
    "skills": skills_sorted
}
with open(output, "w", encoding="utf-8") as f:
    json.dump(out_data, f, indent=2)
print(f"Generated {len(skills_sorted)} skills -> {output} ({output.stat().st_size} bytes)")
