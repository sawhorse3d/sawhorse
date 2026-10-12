# Cabinet generator

A sample Sawhorse plugin (plugin API 1). It builds a frameless (European-style) cabinet from its sizes, with every part a named component, so the Outliner shows the parts and the Cut list reads them straight away.

## Use it

1. Open the Plugins pane (the puzzle-piece tab at the end of the side panel) and click **Enable…** beside Cabinet generator. The dialog lists what it asks for: read the model, change the model, add a panel and a command, keep settings.
2. The **Cabinet** section appears at the top of the pane. Type the sizes in your document's units (`30`, `72`, `12` with inches; `600`, `720`, `560` with millimetres): width, height, depth, stock thickness, the number of shelves, the back panel and the toe kick height.
3. Click **Generate cabinet** (or Plugins > Cabinet generator > Generate Cabinet, or Alt+Shift+C).

What it makes, x across the front, y front to back, z up:

| Part | Copies | Size |
|---|---|---|
| Side | Right side, and Left side as its mirror image | stock x depth x height, full height |
| Top, Bottom | one each | between the sides, the depth less the back |
| Shelf | Shelf 1 .. Shelf n | as the top, spaced so every opening is the same height |
| Back | one (when ticked) | 1/4 in (6 mm in metric documents) sheet between the sides, from the toe kick up |
| Toe kick | one (when the height is more than 0) | stock thick, set back 3 in (75 mm) from the front |

The sides run to the floor; cut the toe notch in them yourself if you want one. The left side is the right side mirrored (the Cut list notes it as handed), so anything done inside one side, such as a column of shelf pin holes, appears inside the other. The whole cabinet is one undo step ("Generate cabinet").

**Changing it.** Select the cabinet (a click on it selects the whole component) and the button reads **Update selected cabinet**: it replaces that cabinet in place with the new sizes, again as one undo step. With anything else selected, it adds a new cabinet beside the model.

The last sizes are kept for next time (the plugin's own store), and which cabinet it made is kept in the document (saved inside the `.shz`).

## How it is built

`main.js` is plain JavaScript with `// @ts-check`, checked against `packages/plugin-api/index.d.ts` by `npm run plugin:check`. The cabinet is one `model.batch(...)` holding one `batch.component(...)`: each part is a component definition with a box in its own axes, placed by its copies inside a "Cabinet" component. Read it next to `docs/PLUGINS.md`.

## Licence

MIT (see `LICENSE`). Author: Sawhorse.
