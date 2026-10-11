# Changelog

All notable changes to Sawhorse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/) once releases start.

## [Unreleased]

### Added (lane 1, foundations, 2026-10-11)

- Browser app shell built from the approved mock: top bar with menus, Iso/Top/Front/Right/Persp views and Credits chip; tool rail with the SketchUp-2017 default keys and our own glyphs; three.js WebGL2 viewport (Z-up, grid, axes, coarse hover inference in our four colours); Outliner / Tags / Cut list / Render panel; Value Control Box with fractional in, decimal in, ft-in, mm, cm and m. Light and dark themes.
- Tauri 2.x desktop scaffold wrapping the same front end (not yet built locally; CI builds installers).
- Rust workspace: `kernel`, `tessellate`, `inference`, `export`, `wasm-api`. Every crate builds for `wasm32-unknown-unknown`.
- Kernel: half-edge B-rep with hole loops and stored planes; named tolerances; topology validator (integrity, loop closure, twins, planarity, winding, self-intersection, hole containment, Euler-Poincare per shell, watertight); sketches with automatic region detection; extrude to a watertight solid; push/pull for faces whose neighbours run parallel to the push; transactional ops with typed errors; deterministic hashing.
- Booleans behind a `BooleanEngine` trait with a manifold-rust adapter that keeps per-face provenance.
- Watertight-only binary STL and 3MF exporters.
- Kernel running as WebAssembly in a Web Worker; Push/Pull and Rect typed values drive it end to end.
- Tests: property invariants, an op-sequence fuzzer with repro files, golden-hash replays checked natively and in WASM, a cargo-fuzz harness, Vitest and Playwright suites.
- CI on GitHub Actions and a tag-triggered, unsigned-by-default release workflow for Windows, macOS and Linux.
- Docs: ADRs 0001-0004, development guide, clean-room log, dual MIT / Apache-2.0 licence.
