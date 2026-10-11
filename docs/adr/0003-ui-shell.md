# ADR-0003: UI shell

- Status: Accepted, 2026-10-11. Mock approved by the maintainer: "mock looks great".
- Deciders: the maintainer
- Source: `docs/DECISIONS.md` seed; approved mock `docs/mock/sawhorse-mock.html` (published at https://(mock link removed))

## Context

People who already model furniture have SketchUp in their hands: the keys, the push/pull gesture, typing a length mid-gesture. Sawhorse should feel familiar without copying anyone's look, and it must work as a zero-install browser demo as well as a desktop app.

## Decision

- Build exactly the mock: https://(mock link removed). Local copy at `docs/mock/sawhorse-mock.html`.
- Layout: top bar (menus, view buttons Iso/Top/Front/Right/Persp, Credits chip); left tool rail with SketchUp-2017 default keys (Space select, E erase, L line, R rect, C circle, A arc, P push/pull, M move, Q rotate, S scale, F offset, W follow-me, T tape, D dimension, B paint); viewport; right panel tabs Outliner / Tags / Cut list / Render; bottom Value Control Box (typed length/angle/count mid-gesture; units in, ft-in, fractional in, mm, cm, m).
- Inference colours are OURS: endpoint lime, midpoint teal, on-edge amber, on-face violet. Not SketchUp's.
- Own glyphs, own layout. No copied icons, no pixel-for-pixel UI (Tetris v. Xio).
- Stack: Tauri 2.x (stay off 3.0 alpha), Vite, React, TypeScript, three.js (WebGL2; WebGPU behind a flag), Vitest, Playwright. Browser build is first-class (the zero-install demo).

## Consequences and implementation status (lane 1)

- Components: `TopBar`, `ToolRail`, `Viewport`, `SidePanel`, `ValueBar` in `src/components/`. Tokens (`src/styles/tokens.css`) and layout CSS (`src/styles/app.css`) are copied from the mock with the same class names, so the two can be diffed. Fonts (Barlow Condensed, IBM Plex Sans, IBM Plex Mono) are self-hosted through `@fontsource` so the desktop app works offline.
- Light and dark follow `prefers-color-scheme`; `?theme=light|dark` forces one (screenshots, demos).
- Viewport: three.js WebGL2, Z-up metres like the kernel, orthographic iso-feel default camera (eye in the -X/-Y/+Z octant), Top/Front/Right ortho views and a Persp view; 6 in grid; RGB axes; faces coloured with the mock's three face tokens by normal; edges from the kernel. Render on demand only. Middle button orbits, Shift+middle and right button pan, wheel zooms; the left button belongs to tools.
- Coarse hover inference (screen space, edges occlusion-tested) shows OUR four colours with the mock's tip and legend. The real inference engine lives in `crates/inference` (inference lane).
- Value Control Box: always visible, as the approved mock renders it (its `hidden` toggle never took effect because `.vcb label` sets `display:flex`). Tools that take no value show it read-only as "Measurements". Typing a digit, `.`, `-`, `/`, `'`, `"` or `,` while a value tool is active jumps into the box (SketchUp-style "just type"). Units cycle from the unit label; a typed suffix (`mm`, `cm`, `m`, `in`, `"`, `ft`, `'`) always wins.
- Lane-1 wiring: Push/Pull (P) + distance and Rect (R) + `w,h` drive the kernel; other value tools say which lane brings them.
- Side panel panes carry the mock's example content until their lanes land. The Credits chip shows 0 and the Render pane says the service is not connected (no fake balance, nothing charged).
- Tauri 2 scaffold in `src-tauri/` (2.12.3 resolved), guarded by `scripts/check-tauri-version.mjs`.
