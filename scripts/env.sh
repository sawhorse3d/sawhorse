# Source me: `. scripts/env.sh`
# Puts cargo on PATH and points CARGO_TARGET_DIR at regenerable bulk storage off C:.
# Never commit a target-dir into .cargo/config.toml; this file is the one place it lives.

if [ -d "$HOME/.cargo/bin" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; fi
export PATH

# Every checkout gets its own target subdir, named for its folder and keyed by a checksum of
# its full path: a lane in a git worktree (<repo>/.worktrees/<slug>) gets "<slug>-<sum>", any
# other checkout "<folder>-<sum>". Parallel lanes never queue on each other's cargo lock or
# evict each other's artifacts, and two clones never share one. Sharing is not just slow, it
# is wrong: cargo copies the unhashed wasm cdylib out of deps/ only when it compiles, so a
# checkout cargo thinks is fresh gets whatever kernel the other one copied out last (the
# 2026-10-12 stale-kernel trap; scripts/wasm-checkout.mjs now catches it too).
_sawhorse_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd)"
case "$_sawhorse_root" in
  */.worktrees/*) _sawhorse_name="${_sawhorse_root##*/.worktrees/}"; _sawhorse_name="${_sawhorse_name%%/*}" ;;
  *) _sawhorse_name="${_sawhorse_root##*/}" ;;
esac
_sawhorse_sum="$(printf '%s' "$_sawhorse_root" | cksum | cut -d' ' -f1)"
_sawhorse_lane="-${_sawhorse_name}-${_sawhorse_sum}"

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  if grep -qi microsoft /proc/version 2>/dev/null && [ -d /mnt/d ]; then
    # WSL: same D: cache, separate subdir (Linux host artifacts must not mix with Windows ones).
    export CARGO_TARGET_DIR="/mnt/d/build-cache/sawhorse/wsl${_sawhorse_lane}"
  else
    case "$(uname -s 2>/dev/null)" in
      MINGW*|MSYS*|CYGWIN*) export CARGO_TARGET_DIR="D:/build-cache/sawhorse${_sawhorse_lane}" ;;
      # CI and other machines keep cargo's default (crates/target, gitignored).
    esac
  fi
fi
unset _sawhorse_root _sawhorse_name _sawhorse_sum _sawhorse_lane
