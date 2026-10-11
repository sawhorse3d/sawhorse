# Fuzzer repro sequences

`tests/sequences.rs` writes a failing, shrunk op sequence here as `pending-<test>.json`.

1. Rename it to say what it found (e.g. `2026-10-12-pushpull-hole-wall-cross.json`).
2. Fix the kernel until `cargo test --test repro` passes.
3. Commit the file with the fix. `tests/repro.rs` replays every `*.json` here on every run,
   so the bug can never come back silently.

Format: `{ "name", "error", "ops": [ ...document op log, any op including "undo" and "redo"... ] }`,
lengths in metres. The same files load into the WASM kernel (`document_replay(JSON.stringify(ops))`) for debugging in the browser.
