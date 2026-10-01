# Local usage ledger

`nur ledger` aggregates usage from Nur, Claude Code, Codex, Droid, Pi, Devin, and Command Code on Windows, macOS, and Linux. It needs no chat login or provider key. `/ledger [today|7|month|all]` opens the same report in the TUI without blocking the editor.

```bash
nur ledger                         # this calendar month
nur ledger --period today
nur ledger --period 7 --json        # trailing seven local calendar days
nur ledger --period all
nur ledger --refresh-prices         # fetch public LiteLLM rates explicitly
```

The report groups requests by agent, provider, and model, with fresh input, output, cache reads, cache writes, and reasoning tokens. Reported charges and estimates have separate columns. Unpriced requests stay unpriced. Reasoning tokens are included in output and are not added twice. API estimates do not represent the price of a subscription. Droid's cumulative session totals use its last-active date because its settings files do not provide request dates.

Nur's recorded price estimates are retained. For other logs, estimates use `~/.nur/ledger/prices.json` after an explicit refresh. Missing cache rates leave a request unpriced. Reports work offline; refreshing prices downloads a public rate table and sends no logs, keys, account details, or usage.

The parser reads supported usage logs and Devin's database in read-only mode. It retains only numeric counts, dates, hashed request identities, and model/provider labels under `~/.nur/ledger/usage-cache.json`. It never imports credentials, prompt text, tool results, or account files. Streamed Claude updates and repeated Codex cumulative counters are deduplicated. Cached numeric history survives rotated logs. `--home <directory>` provides an explicit log root and ignores machine-local Claude/Codex root overrides, useful for fixtures or an alternate home.

This cross-platform integration is informed by the MIT-licensed [Token Ledger](https://github.com/bisheshabramhacharya/token-ledger) parsers. Its license is preserved in `third-party/token-ledger-LICENSE`. The upstream macOS menu-bar app remains independently available.
