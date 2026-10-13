#!/usr/bin/env bash
# Eight parallel shards of the op-sequence fuzzer, then one summary line per shard and the
# boolean refusal counts. Runs where cargo runs (WSL on the maintainer's machine):
#
#   MSYS_NO_PATHCONV=1 wsl.exe bash /mnt/c/.../scripts/fuzz-shards.sh [cases-per-shard] [seed-base] [snap 1|0]
#
# From Git Bash keep MSYS_NO_PATHCONV=1 (Git Bash rewrites /mnt/... arguments otherwise).
# Defaults: 6250 per shard (50,000 in all), seeds 20261101..20261108, plane snapping on.
# Logs: /tmp/sawhorse-fuzz-<seed>.log. A failing shard writes its shrunk sequence to
# crates/kernel/tests/repro/pending-long_fuzz_seed<seed>.json, or to $FUZZ_REPRO_DIR when it
# is set (a run that should leave the checkout alone: FUZZ_REPRO_DIR=/tmp/sawhorse-repro).
set -u
cases="${1:-6250}"
base="${2:-20261100}"
snap="${3:-1}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here/crates"
# shellcheck source=/dev/null
. ../scripts/env.sh
cargo test -p sawhorse-kernel --test sequences --no-run >/dev/null 2>&1 || { echo "build failed"; exit 1; }
start=$(date +%s)
for s in 1 2 3 4 5 6 7 8; do
  seed=$((base + s))
  FUZZ_REPRO_DIR="${FUZZ_REPRO_DIR:-}" FUZZ_PLANE_SNAP=$snap FUZZ_REPORT=1 FUZZ_CASES=$cases FUZZ_SEED=$seed \
    cargo test -p sawhorse-kernel --test sequences -- --ignored long_fuzz --nocapture > "/tmp/sawhorse-fuzz-$seed.log" 2>&1 &
done
wait
echo "wall $(( $(date +%s) - start )) s (plane snapping $([ "$snap" = 1 ] && echo on || echo off))"
for s in 1 2 3 4 5 6 7 8; do
  f="/tmp/sawhorse-fuzz-$((base + s)).log"
  grep -h "long_fuzz_seed\|minimal failing sequence" "$f"
done
cat /tmp/sawhorse-fuzz-$((base + 1)).log /tmp/sawhorse-fuzz-$((base + 2)).log /tmp/sawhorse-fuzz-$((base + 3)).log \
    /tmp/sawhorse-fuzz-$((base + 4)).log /tmp/sawhorse-fuzz-$((base + 5)).log /tmp/sawhorse-fuzz-$((base + 6)).log \
    /tmp/sawhorse-fuzz-$((base + 7)).log /tmp/sawhorse-fuzz-$((base + 8)).log |
  awk '/not a valid solid\): /{v+=$NF} /degenerate engine output\): /{d+=$NF} END {print "boolean-path refusals: not a valid solid " v ", degenerate " d}'
