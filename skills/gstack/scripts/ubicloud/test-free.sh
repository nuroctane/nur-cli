#!/usr/bin/env bash
# bun run test:ubicloud [test:free args] — run the free suite on an ephemeral
# Ubicloud VM (standard-16 unless UBI_SIZE says otherwise) with the required CI
# lane's environment. Shard logs land in .context/ubicloud/<timestamp>/.
# With --record-durations, the refreshed scripts/free-test-durations.json is
# copied back into this checkout.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
logs="$root/.context/ubicloud/$(date +%Y%m%d-%H%M%S)"

args=(--src "$root" --setup "$here/setup-free-suite.sh"
  --env GSTACK_EXPECT_BINARIES=1 --env GSTACK_FREE_RETRY_FLAKY=1
  --env GSTACK_FLAKE_LEDGER=/tmp/gstack-free-test-flake-ledger.jsonl
  --pull "/tmp/gstack-free-test-*:$logs"
  --pull "work/$(basename "$root")/.context/free-test-logs:$logs")
[ -z "${GSTACK_FREE_JOBS:-}" ] || args+=(--env "GSTACK_FREE_JOBS=$GSTACK_FREE_JOBS")
for arg in "$@"; do
  [ "$arg" != --record-durations ] || args+=(--pull "work/$(basename "$root")/scripts/free-test-durations.json:$root/scripts")
done

"$here/ubi-runner.sh" run "${args[@]}" -- "xvfb-run -a bun run test:free $*"
