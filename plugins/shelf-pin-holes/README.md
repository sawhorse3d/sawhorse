# Shelf pin holes

A sample Sawhorse plugin (plugin API 1): a tool that drills a column of shelf pin holes into a face, the way a line-boring jig does on a cabinet side.

## Use it

1. Enable it in the Plugins pane (it asks to read and change the model, add a tool and keep its settings).
2. Pick **Pins** in the rail's plugin group (the last group of tools), or press Alt+Shift+P.
3. Hover a face. A column of holes shows, starting at the cursor and going up the face (along the length of a level face).
4. Click: the column is drilled. One Ctrl+Z takes the whole column back.

**The Value Control Box** takes `count, pitch, diameter`: `8, 32mm, 5mm` to start (the European 32 mm system with 5 mm pins). Any part may be left out: `12` changes only the count, `, 37mm` only the pitch. Esc clears the preview. The settings are kept for next time.

**Depth.** 3/8 in in inch documents and 10 mm in metric ones, never more than 60% through the part.

**Inside a component.** Holes go into the solid you can edit. For a cabinet side that is a component inside a cabinet, double-click the cabinet, then the side, then drill: every copy of that side gets the holes (Make Unique first to drill one side only).

Each hole is the kernel's own work: a circle drawn on the face (a 16-sided polygon) and pushed in, so the solid stays valid and watertight and the cut list stays right. If the column would run off the face, nothing changes and the status line says why.

## How it is built

`main.js` registers one tool with `ui.registerTool`. It receives pointer events with the inference result (`snap.surface` is the face under the cursor, with its plane), draws the preview with `tool.setView({ overlay, labels, status, busy })`, and drills with one `model.batch(...)`: for each hole a `draw` op on the face, then a `push_pull` op on `hole.face`, the face that draw made (a placeholder filled in when the batch is applied). Read it next to `docs/PLUGINS.md`.

## Licence

MIT (see `LICENSE`). Author: Sawhorse.
