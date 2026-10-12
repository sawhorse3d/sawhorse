# Third-party material

What Sawhorse ships or uses that it did not write, and where its own assets come from. Dependencies are listed with their licences in `package.json`, `crates/Cargo.toml` and `crates/Cargo.lock`; this page is for what a licence list does not say.

## Texture images (lane 10)

**None.** Every texture in the built-in material library (twelve woods: White oak, Red oak, Walnut, Cherry, Hard maple, Ash, Birch, Poplar, Pine, Douglas fir, Mahogany, Western red cedar; eight sheet goods: Baltic birch ply, Maple ply, Red oak ply, MDF, Hardboard, White melamine, OSB, Particle board) is drawn by Sawhorse's own code, `crates/export/src/library.rs`, when it is first used: value noise on a periodic integer lattice for the grain, rings, pores, speckle, chips and strands, written as PNG by `crates/export/src/png.rs`. The colours, sizes and parameters were chosen by eye for a woodworker's model. No photograph, scan or image file from any source (including Poly Haven, ambientCG or any other CC0 library, and SketchUp's own materials) is in the repository or downloaded at run time, so there is no attribution to record. If a CC0 image is ever added, record it here: the file, its source URL, the author, the licence and the date it was fetched.

The sixteen flat colours and the glass and metal basics are plain sRGB values chosen by eye.

## Other assets

| What | Where | Licence / source |
|---|---|---|
| Fonts (Barlow Condensed, IBM Plex Sans, IBM Plex Mono) | `@fontsource/*` npm packages | SIL Open Font License 1.1 |
| Tool glyphs | `src/components/glyphs.tsx` | Our own (ADR-0003) |
| `.skp` reading | npm `openskp` 1.3.0 | MIT (see `docs/CLEANROOM.md` for what was read of it) |
| Parsing plugin sources before they run (lane 11) | npm `acorn` 8.19.0, used as a library through its documented `parse` function; no dependencies | MIT |
| PNG compression | `flate2` and `crc32fast` (MIT OR Apache-2.0) with the `zlib-rs` backend (Zlib licence); all three already used by `zip` | MIT OR Apache-2.0; Zlib |
