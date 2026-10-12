# Source me: `. scripts/env.sh`
# Puts cargo on PATH and points CARGO_TARGET_DIR at regenerable bulk storage off C:.
# Never commit a target-dir into .cargo/config.toml; this file is the one place it lives.

if [ -d "$HOME/.cargo/bin" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; fi
export PATH

# A lane working in a git worktree (<repo>/.worktrees/<slug>) gets its own target subdir,
# so parallel lanes never queue on each other's cargo lock or evict each other's artifacts.
_sawhorse_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd)"
case "$_sawhorse_root" in
  */.worktrees/*) _sawhorse_lane="-${_sawhorse_root##*/.worktrees/}"; _sawhorse_lane="${_sawhorse_lane%%/*}" ;;
  *) _sawhorse_lane="" ;;
esac

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
unset _sawhorse_root _sawhorse_lane
