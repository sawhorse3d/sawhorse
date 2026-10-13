# Draft upstream issue: openskp 1.3.0 refuses a SketchUp 2018 save

Status: a draft for the maintainer to file on openskp's issue tracker (not filed by the build). Written 2026-10-12 (lane 15). Nothing here comes from openskp's source code: it is what a user of the published npm package sees.

---

**Title:** `parseSkp` refuses a SketchUp 2018 file: "legacy .skp parse failed: base probe: anchor resolved to null"

**Version:** openskp 1.3.0 (npm), Node 24.18.0 and Chromium-based browsers (the same in both).

**What happens.** Parsing a file saved by SketchUp 2018 (version string `{18.0.16975}`) throws:

```
legacy .skp parse failed: base probe: anchor resolved to null | stage=legacy_walk
```

**To reproduce.** With openskp's own test file `zero_material_custom_layer_v18.skp` (8,475 bytes):

```js
import { readFileSync } from "node:fs";
import { parseSkp } from "openskp";
const b = readFileSync("zero_material_custom_layer_v18.skp");
parseSkp(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)); // throws
```

**Expected.** The file parses, as SketchUp 2014, 2017 (`{17.0.1}`, `{17.0.18899}`) and 2020 (`{20.1.235}`) saves do with the same version of the package; the README lists 2013 and later as supported.

**Also seen.** The package's synthetic SketchUp 3, 6 and 7 files are refused with "expected a string record", "back-ref to unwalked slot" and "definition list misaligned". Those are outside the stated range, so no change is asked for them; a clearer message ("SketchUp 2012 and older are not supported") would help users.

**Why it matters to us.** Sawhorse (a free, MIT OR Apache-2.0 modeler) reads `.skp` through openskp and shows its message unchanged; 2018 saves are still common in the files woodworkers share. Happy to test a fix against a set of real-world files.

---

Where Sawhorse documents this: [docs/skp-compat.md](../skp-compat.md) (file versions), [docs/LIMITS.md](../LIMITS.md).
