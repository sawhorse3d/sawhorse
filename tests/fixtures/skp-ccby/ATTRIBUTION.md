# SketchUp fixtures by Corynth (CC BY 4.0)

The five `.skp` files in this folder are real SketchUp-authored furniture models, used as public test fixtures for Sawhorse's `.skp` import, cut list and nesting (`e2e/skp-ccby.spec.ts`). They are the only SketchUp-authored files in this repository.

- **Author:** Corynth
- **Source:** [Corynth's free assets: SketchUp 3D assets](https://corynth.itch.io/free-assets-sketchup-3d-assets) (the download `SketchUp_20240130100829.zip`, folder `Corynth's Stuff/share/`)
- **Licence:** [Creative Commons Attribution 4.0 International (CC BY 4.0)](https://creativecommons.org/licenses/by/4.0/), as stated on the source page
- **Changes:** none. The files are byte for byte as downloaded (SHA-256 below); only these five were taken from the download.
- **Saved by:** SketchUp 2023 (version stamp `{23.2.0}`)

| File | Bytes | SHA-256 |
|---|---|---|
| `benches.skp` | 135,766 | `c9f1cc7486aacf897e3d9d44107954bad0d6b4ba20012192e47a4b8cddacc994` |
| `coffee tables.skp` | 250,192 | `91d7321550b78147cda25b436275e3ae1ac60ae39bf52b7c6f1b8def7ef0359d` |
| `cupboard.skp` | 238,123 | `63431b71863688994d987e9dc90f7d369df336ecafc9a7cca2db4018432c372e` |
| `desk.skp` | 169,271 | `6d370f9ce838b5a0c0abde9271cec7e7850f895f88a8438e8d5cbd50ce909301` |
| `side table.skp` | 150,333 | `eddaf9709e8027e419207d5507a7bba3085b9c5f1dd4f322b46489aa4c28bf99` |

They are test inputs only: they are not part of the app, the desktop installers or the public site (`scripts/check-provenance.mjs` and `scripts/check-site.mjs` refuse SketchUp files in either). The spec checks their SHA-256 before it uses them, so a changed file fails rather than passing silently.

Thank you, Corynth.
