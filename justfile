# Sawhorse task runner. `just --list` for everything.
# Rust recipes go through scripts/cargo.sh, which forwards to WSL on a Windows box
# without the MSVC linker (see docs/DEVELOPMENT.md).

set shell := ["bash", "-cu"]
set windows-shell := ["bash", "-cu"]

# Regenerable build bytes stay off C: (ADR build rules). WSL picks its own subdir in scripts/env.sh.
export CARGO_TARGET_DIR := env_var_or_default("CARGO_TARGET_DIR", if os() == "windows" { "D:/build-cache/sawhorse" } else { justfile_directory() / "crates/target" })

default:
    @just --list

# Day-1 spike: every crate must build for the browser.
check-wasm:
    scripts/cargo.sh check --workspace --target wasm32-unknown-unknown

# Native type check of the workspace.
check:
    scripts/cargo.sh check --workspace --all-targets

# Unit, property and replay tests (debug: validator runs after every mutation).
test:
    scripts/cargo.sh test --workspace

# Long sequence-fuzz run (writes failures to crates/kernel/tests/repro/).
fuzz-long cases="20000":
    scripts/cargo.sh test -p sawhorse-kernel --test sequences -- --ignored --nocapture long_fuzz -- {{cases}}

# Rewrite golden hashes after an intended kernel change (review the diff!).
golden-update:
    UPDATE_GOLDEN=1 scripts/cargo.sh test -p sawhorse-kernel --test golden

fmt:
    cd crates && cargo fmt --all

fmt-check:
    cd crates && cargo fmt --all -- --check

clippy:
    scripts/cargo.sh clippy --workspace --all-targets -- -D warnings

# Build the kernel WASM into src/wasm/pkg.
wasm:
    npm run wasm

dev:
    npm run dev

build:
    npm run build:all

web-test:
    npx vitest run

# Playwright against the production build; locally uses the installed Edge (no browser download).
e2e:
    PW_CHANNEL="${PW_CHANNEL:-msedge}" REQUIRE_KERNEL=1 npx playwright test

# The Phase-2 exit test alone (prints its time, volume and refusal tally).
bookshelf:
    PW_CHANNEL="${PW_CHANNEL:-msedge}" REQUIRE_KERNEL=1 npm run e2e:bookshelf

# What CI runs, in order.
ci: fmt-check clippy test check-wasm wasm build web-test
