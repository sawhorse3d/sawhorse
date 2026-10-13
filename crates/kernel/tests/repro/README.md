# Fuzzer repro sequences

`tests/sequences.rs` writes a failing, shrunk op sequence to `pending/pending-<test>.json`
(for a seeded long run, `pending/pending-long_fuzz_seed<N>.json`). The `pending/` directory
is gitignored and `tests/repro.rs` never reads it, so a fresh find cannot break
`replay_all_repros` for anyone else before it is fixed. A `pending-*.json` left in this
directory itself is skipped too, with a loud warning.

To promote a find to a regression case:

1. Move it up into this directory under a name that says what it found
   (e.g. `2026-10-12-pushpull-hole-wall-cross.json`), and set its `"name"` to match.
   Keep `"plane_snap": false` if it is there: the find was made with `FUZZ_PLANE_SNAP=0`,
   and the replay runs it with plane snapping off too.
2. Fix the kernel until `cargo test --test repro` passes.
3. Commit the file with the fix. `tests/repro.rs` replays every `*.json` here on every run,
   so the bug can never come back silently.

A pending find that no longer fails on current `main` (fixed by other work) is deleted,
with a line in the commit that says so.

Format: `{ "name", "error", "plane_snap"?, "ops": [ ...document op log, any op including "undo" and "redo"... ] }`,
lengths in metres. Never put a machine path in a file here. The same files load into the
WASM kernel (`document_replay(JSON.stringify(ops))`) for debugging in the browser.
