# Developing Sawhorse

Three places run the code: the maintainer's Windows machine (no MSVC), WSL Ubuntu on that machine (all Rust work), and GitHub Actions (everything, plus the installers). Commands below are exact; run them from the repo root (`%USERPROFILE%\Desktop\sawhorse\app`) in Git Bash unless a section says otherwise.

## The two rules that shape everything

1. **No MSVC on the Windows machine.** Visual Studio Build Tools are a multi-gigabyte download on a metered link, so they are not installed. Without `link.exe`, Rust cannot even `cargo check` on Windows: build scripts and proc-macros (serde_derive, thiserror, wasm-bindgen-macro) are host programs that must be linked. Git Bash's `/usr/bin/link` (coreutils) shadows the name and fails with `link: extra operand`. Therefore:
   - every Rust command runs in **WSL Ubuntu**; `scripts/cargo.sh` and `npm run wasm` forward there automatically when they see Windows without MSVC;
   - native Windows, macOS and Linux installers are built by **GitHub Actions** (`.github/workflows/release.yml`, `tauri-action`);
   - `tauri dev` / `tauri build` do not run locally until a linker exists.
2. **Build bytes live on D:.** `CARGO_TARGET_DIR` is `D:/build-cache/sawhorse` on Windows and `/mnt/d/build-cache/sawhorse/wsl` in WSL (separate subdir: Linux host artifacts must not mix with Windows ones). `scripts/env.sh` and the `justfile` set it. Never commit a `target-dir` into `.cargo/config.toml`. CI uses cargo's default (`crates/target`, gitignored).

## Toolchains

| Where | What | Notes |
|---|---|---|
| Windows | Node 24 + npm 11 | front end, Vite, Vitest, Playwright |
| Windows | Rust stable 1.99 at `%USERPROFILE%\.cargo\bin`, `wasm32-unknown-unknown` target | only useful once MSVC exists; `export PATH="$HOME/.cargo/bin:$PATH"` |
| Windows | `just` | optional task runner (`just --list`) |
| WSL Ubuntu 24.04 | rustup stable 1.99 + `wasm32-unknown-unknown` + `rustfmt` + `clippy`, gcc, Node 22 | does all Rust work |
| WSL Ubuntu | `wasm-pack` 0.15.0 in `~/.local/bin` | installed with `npm i -g --prefix ~/.local wasm-pack@0.15.0` |

One-time WSL setup (already done on the maintainer's machine, 2026-10-11):

```bash
wsl.exe bash -lc 'rustup default stable && rustup target add wasm32-unknown-unknown && rustup component add rustfmt clippy'
wsl.exe bash -lc 'npm i -g --prefix ~/.local wasm-pack@0.15.0'
```

## Daily loop on Windows

```bash
npm install                 # once, or after package.json changes
npm run wasm                # builds the kernel to src/wasm/pkg (runs wasm-pack inside WSL)
npm run dev                 # http://localhost:5173 - the browser build, kernel included
```

Without `npm run wasm` the app still starts; the viewport HUD says "Kernel WASM is not built. Run: npm run wasm".

Front-end checks:

```bash
npx tsc -b                  # types
npx vitest run              # unit + component tests (jsdom)
npm run build               # tsc + vite production build into dist/
npm run build:all           # wasm + build (what Tauri's beforeBuildCommand runs)
PW_CHANNEL=msedge REQUIRE_KERNEL=1 npx playwright test   # e2e on the installed Edge, no browser download
npm run check:tauri         # fails if Tauri resolved to anything but 2.x
```

`?theme=light` or `?theme=dark` on the URL forces a theme.

Lane-end capture (every lane ends with one, saved beside the repo in `../_captures/`), with `npm run dev` running:

```bash
node scripts/capture.mjs --out ../_captures/2026-10-11-lane01-shell.png --theme light --tool p --type "3/4" --hover endpoint
```

It drives the installed Edge through Playwright, waits for the kernel, applies the typed value, hovers a visible corner (via the read-only `window.__sawhorse.visibleEndpoints()` hook) and prints the kernel state it captured.

## Rust (from Git Bash on Windows, forwarded to WSL)

`scripts/cargo.sh <args>` runs `cargo <args>` in `crates/`, inside WSL when needed (it also forwards `UPDATE_GOLDEN`, `FUZZ_CASES`, `PROPTEST_CASES`, `RUST_BACKTRACE`, `RUSTFLAGS`).

```bash
bash scripts/cargo.sh check --workspace --target wasm32-unknown-unknown   # day-1 spike: everything builds for the browser
bash scripts/cargo.sh test --workspace                                     # all tests (debug: validator after every mutation)
bash scripts/cargo.sh clippy --workspace --all-targets -- -D warnings
bash scripts/cargo.sh fmt --all                                            # rustfmt (not installed on Windows)
FUZZ_CASES=50000 bash scripts/cargo.sh test -p sawhorse-kernel --test sequences -- --ignored long_fuzz
UPDATE_GOLDEN=1 bash scripts/cargo.sh test -p sawhorse-kernel --test golden # accept an intended hash change; review the diff
npm run golden:wasm                                                        # after npm run wasm: WASM hashes == native goldens
```

The same through `just`: `just check-wasm`, `just test`, `just clippy`, `just fmt`, `just fuzz-long`, `just golden-update`, `just wasm`, `just e2e`, `just ci`.

## Rust directly inside WSL

```bash
wsl.exe
cd /mnt$HOME/Desktop/sawhorse/app
. scripts/env.sh            # cargo + ~/.local/bin on PATH, CARGO_TARGET_DIR=/mnt/d/build-cache/sawhorse/wsl
cd crates
cargo test --workspace
cargo check --workspace --target wasm32-unknown-unknown
cd .. && wasm-pack build crates/wasm-api --target web --out-dir ../../src/wasm/pkg --out-name sawhorse_wasm --no-pack --release
```

## Test machinery (ADR-0001 "robustness first")

| What | Where | Runs |
|---|---|---|
| Validator after every B-rep and sketch mutation | `Brep::debug_validate`, `Sketch::check` | every debug build (tests) |
| Validate before commit | `Model::apply` (clone, mutate, validate, commit) | always |
| Property invariants | `crates/kernel/tests/invariants.rs` | `cargo test` |
| Op-sequence fuzzer | `crates/kernel/tests/sequences.rs` (`quick_fuzz` 384 sequences; `long_fuzz` ignored) | `cargo test`; CI runs 10,000 |
| Repro replay | `crates/kernel/tests/repro/*.json` via `tests/repro.rs` | `cargo test` |
| Golden hashes | `crates/kernel/tests/golden/*.json` via `tests/golden.rs` and `scripts/golden-wasm.mjs` | `cargo test`, `npm run golden:wasm` |
| cargo-fuzz | `crates/kernel/fuzz` (own workspace, nightly + libFuzzer) | `cd crates/kernel && cargo +nightly fuzz run op_sequence` (Linux/WSL) |

When the fuzzer fails it writes the shrunk sequence to `crates/kernel/tests/repro/pending-<test>.json`. Rename it, fix the kernel, commit both; see `crates/kernel/tests/repro/README.md`.

## Desktop (Tauri 2)

`src-tauri/` is configured (`tauri.conf.json`, capabilities, icons from `src-tauri/app-icon.svg`) but not built locally. CI's `tauri-check` job compiles it on Ubuntu; tagging `v*` runs `release.yml`, which builds Windows, macOS and Linux installers into a draft prerelease, unsigned unless signing secrets exist. Once a machine has a linker: `npm run tauri dev`.

Stay on Tauri 2.x stable. 3.0 is alpha; `npm run check:tauri` fails on anything else.

## CI (`.github/workflows/ci.yml`)

- `rust` (ubuntu-latest): `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, 10,000-sequence fuzz, wasm32 check.
- `web` (ubuntu-latest): `npm ci`, Tauri guard, `wasm-pack` build, golden cross-check, `npm run build`, Vitest, Playwright Chromium e2e with `REQUIRE_KERNEL=1`.
- `tauri-check` (ubuntu-22.04): `cargo check --locked` in `src-tauri` with webkit2gtk.

No secrets are needed; no auto-merge is configured.

## Troubleshooting

- `link: extra operand ... rcgu.o` on Windows: that is Git's coreutils `link`, not MSVC. Use `scripts/cargo.sh` (WSL).
- `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)` from a Node script on Windows: a Node 24 teardown bug when calling `process.exit()`; our scripts set `process.exitCode` instead.
- Viewport says WebGL2 is unavailable: hardware acceleration is off in that browser.
- `cargo` missing in WSL: `wsl.exe bash -lc 'rustup default stable'`.
