# Source me: `. scripts/env.sh`
# Puts cargo on PATH and points CARGO_TARGET_DIR at regenerable bulk storage off C:.
# Never commit a target-dir into .cargo/config.toml; this file is the one place it lives.

if [ -d "$HOME/.cargo/bin" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
if [ -d "$HOME/.cargo/bin" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; fi
export PATH

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  if grep -qi microsoft /proc/version 2>/dev/null && [ -d /mnt/d ]; then
    # WSL: same D: cache, separate subdir (Linux host artifacts must not mix with Windows ones).
    export CARGO_TARGET_DIR=/mnt/d/build-cache/sawhorse/wsl
  else
    case "$(uname -s 2>/dev/null)" in
      MINGW*|MSYS*|CYGWIN*) export CARGO_TARGET_DIR=D:/build-cache/sawhorse ;;
      # CI and other machines keep cargo's default (crates/target, gitignored).
    esac
  fi
fi
