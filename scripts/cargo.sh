#!/usr/bin/env bash
# Run cargo for the crates/ workspace, anywhere.
#
# On Windows without the MSVC linker (the maintainer's machine), even `cargo check` fails:
# build scripts and proc-macros are host binaries that must be linked. So on Windows
# this forwards to WSL Ubuntu, which has gcc. Elsewhere it runs cargo directly.
#   scripts/cargo.sh check --workspace --target wasm32-unknown-unknown
#   scripts/cargo.sh test --workspace
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

needs_wsl() {
  case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) ;;
    *) return 1 ;;
  esac
  # A real MSVC link.exe makes native Windows builds possible; Git's /usr/bin/link is not it.
  if command -v cl.exe >/dev/null 2>&1; then return 1; fi
  [ "${SAWHORSE_NO_WSL:-}" = "1" ] && return 1
  [ -n "${CI:-}" ] && return 1
  return 0
}

if needs_wsl; then
  win="$(cd "$here" && pwd -W)"            # C:/Users/...
  drive="$(printf '%s' "${win:0:1}" | tr 'A-Z' 'a-z')"
  wslpath="/mnt/${drive}${win:2}"
  args=""
  for a in "$@"; do args+=" $(printf '%q' "$a")"; done
  # Environment does not cross into WSL by itself; forward the knobs the tests read.
  envs=""
  for v in UPDATE_GOLDEN FUZZ_CASES FUZZ_SEED FUZZ_REPORT FUZZ_PLANE_SNAP FUZZ_REASON_LEN FUZZ_DUMP_REFUSALS PROPTEST_CASES RUST_BACKTRACE RUSTFLAGS; do
    if [ -n "${!v:-}" ]; then envs+="$v=$(printf '%q' "${!v}") "; fi
  done
  exec wsl.exe bash -lc "cd '$wslpath/crates' && . ../scripts/env.sh && ${envs}cargo$args"
fi

# shellcheck source=/dev/null
. "$here/scripts/env.sh"
cd "$here/crates"
exec cargo "$@"
