# Watermarking and channel ids (make leaks self-identify)

The single highest-leverage defender technology in this whole family, generalized from the benchmark-solutions pattern (self-reported model/harness) and classic canary tokens.

## The mechanism

Every distributed artifact of potential attacker value carries a unique, invisible-unless-you-correlate marker. When the artifact is USED, the marker tells you which artifact and (via your manifest) which distribution channel leaked it. Identification without any detection heuristics: the attacker performs your attribution for you.

## Marker taxonomy

| Marker type | Use | Carrier |
|-------------|-----|---------|
| Unique constant | Onchain bait (magic values, canary slot numbers, dust amounts) | Decoy contracts, bait wallets |
| Unique string/uuid | Docs, runbooks, "leaked" keys, READMEs | Any text artifact |
| Unique address | Per-channel decoy contract or bait wallet | All of the above |
| Unique timing | Deploy/publish times offset per channel | Publish pipeline |
| Self-report fields | "verify access" steps in decoy APIs/pages asking for environment strings | Decoy endpoints |
| Unique format quirks | Whitespace/typos/field-order per copy of the same doc | Docs, PDFs, pastes |

## Channel manifest (mandatory)

Internal-only table: `artifact id | marker | channel | deployed | expires | watched-by | fires->action`. Rules:

1. No artifact exists without a manifest row. An unmarked lure is an unowned risk.
2. Markers must not be guessable from each other (no sequential ids; no shared prefixes that let an attacker dedupe your whole channel map from one catch).
3. Expect markers to leak about your decoy SET if the attacker compares artifacts - design so one caught marker reveals nothing about others (this is the "they shouldn't know the case id" principle: compromise of one artifact must not compromise the program).
4. Rotate on a schedule; retire fired markers with a post-mortem row.

## Self-reporting lures (serve and learn)

Benchmark-solutions pattern, three flavors:

1. **Ask directly, accept everything**: a "solution"/runbook retrieval that requests model id / harness / operator notes; unrecognized values get a fallback anyway. The biter is informed AND identified.
2. **Behavioral depth**: require 2-3 plausible steps (fetch key file, hit verify endpoint, call decoy contract). Each step is a separate telemetry event; completing the chain is high-confidence attacker classification (S).
3. **Payment-shaped humor, lawful edition**: the community jokes ("resend payment") because drainer aesthetics amuse - implement at most a friction gate (wait/verify), never an actual payment or theft. The joke stays a joke.

## Using marker hits

A fired marker is a high-confidence classification event. Response ladder: `identifying-agentic-attackers` `references/response-ladder.md`. Public disclosure of a fired marker (the deterrence costly-signal) should reveal the MARKER and the FACT of the tripwire, never the channel map or the decoy set size.

## Cross-channel correlation example

An exploit attempt appears using decoy timelock address `0xD00D...` (marker A, channel: fake-runbook-v3) and bait wallet `0xBEEF...` (marker B, channel: leaked-env-gist-2). Two artifacts fired -> the operator runs multi-channel recon -> attribution confidence rises, evidence pack gains two independent threads, and you have learned your poison WORKS (both channels reached live attackers). Update priors, refresh both channels, keep manifest discipline.
