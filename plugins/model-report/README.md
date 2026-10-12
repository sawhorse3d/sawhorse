# Model report

A sample Sawhorse plugin (plugin API 1): a live report of the model in the side panel, exported as Markdown and CSV.

## Use it

1. Enable it in the Plugins pane (it asks to read the model, add a panel and a command, save files through the save dialog, and keep a setting).
2. The **Report** section shows, and follows every change you make:
   - **Counts**: solids, groups, components (definitions), instances, faces, edges;
   - the overall size (width, depth and height of everything), the total volume and the board feet;
   - **Materials**: board feet per material for solid lumber and the number of 4 x 8 sheets for sheet goods, from the cut list;
   - **Warnings**: a group with no solid in it, or with imported geometry that is not solid; a part thinner than 0.5 mm; imported meshes that are not solid; open geometry.
3. **Units**: as the document, inches or millimetres. The choice is remembered.
4. **Export report** (also Plugins > Model report > Export Report…) saves `<model> report.md` and `<model> report.csv`, each through the save dialog, where you choose the place.

## How it is built

`main.js` reads with `model.stats()`, `model.bounds()`, `model.cutList()` and `model.tree()`, writes the panel with `panel.update(...)` (tables and notes), listens to `documentChanged`, `documentOpened` and `unitsChanged`, keeps the units choice with `storage.app`, and saves with `file.save(...)`. It never changes the model: it does not ask for `model.write`, and the app would refuse it if it tried. Read it next to `docs/PLUGINS.md`.

## Licence

MIT (see `LICENSE`). Author: Sawhorse.
