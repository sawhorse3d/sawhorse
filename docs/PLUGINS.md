# Writing Sawhorse plugins

A plugin adds commands, panels and tools to Sawhorse and can read and change the model, through plugin API version 1. It is a folder of plain JavaScript modules with a manifest, it runs in its own sandbox (a Web Worker of its own), and it can do only what its manifest asks for and the person allowed when they enabled it. The same plugin runs in the browser build and in the desktop app.

The three samples in [`plugins/`](../plugins) are complete, small and commented; read one beside this guide:

| Sample | Shows |
|---|---|
| [`cabinet-generator`](../plugins/cabinet-generator) | a panel with lengths in the document's units, one batch building a component of named parts, replacing what it made, app and document storage |
| [`shelf-pin-holes`](../plugins/shelf-pin-holes) | a tool: pointer events with the inference result, a live preview, the Value Control Box, Esc, one undo step per click |
| [`model-report`](../plugins/model-report) | reading the model and the cut list, events, tables, saving Markdown and CSV through the save dialog |

The design behind it is [ADR-0009](adr/0009-plugin-api.md).

## Contents

- [A first plugin](#a-first-plugin)
- [The manifest](#the-manifest)
- [Permissions](#permissions)
- [Lifecycle](#lifecycle)
- [The API](#the-api): [model](#sawhorsemodel), [batches](#changing-the-model-batches), [ui](#sawhorseui), [panels](#panels), [tools](#tools), [units](#sawhorseunits), [geometry](#sawhorsegeometry), [events](#sawhorseevents), [storage](#sawhorsestorage), [file](#sawhorsefile), [net](#sawhorsenet), [log](#log-and-the-developer-console)
- [Ops a batch can hold](#ops-a-batch-can-hold)
- [What a plugin cannot do](#what-a-plugin-cannot-do)
- [Checking, packing, installing](#checking-packing-installing)
- [Publishing](#publishing)

## A first plugin

A folder with two files:

```text
hello-box/
  plugin.json
  main.js
```

`plugin.json`:

```json
{
  "$schema": "https://raw.githubusercontent.com/sawhorse3d/sawhorse/main/packages/plugin-api/manifest.schema.json",
  "id": "org.example.hello-box",
  "name": "Hello box",
  "version": "1.0.0",
  "api": 1,
  "description": "Puts a 12 in cube at the origin.",
  "author": "You",
  "license": "MIT",
  "permissions": ["model.write", "ui"],
  "contributes": { "commands": [{ "id": "cube", "title": "Add a Cube" }], "menus": [{ "command": "cube" }] },
  "entry": "main.js"
}
```

`main.js`:

```js
// @ts-check
import { model, ui } from "sawhorse";

await ui.registerCommand({
  id: "cube",
  async run() {
    const b = model.batch("Add a cube");
    b.box({ min: [0, 0, 0], size: [0.3048, 0.3048, 0.3048], name: "Cube" });
    await b.commit();
    await ui.notify("A 12 in cube.");
  },
});
```

`$schema` is optional and only for your editor (completion and checking of `plugin.json`); the app ignores it. The address above works anywhere. The samples in this repository use the relative path `../../packages/plugin-api/manifest.schema.json`, which works only for a folder under `plugins/`; for a copy of the schema next to your plugin, write `./manifest.schema.json`.

Check it (`npm run plugin:check -- path/to/hello-box` in this repository: the manifest, the code rules and a type check of every `// @ts-check` file against the API), zip the folder (`npm run plugin:pack -- path/to/hello-box`, or any zip tool), then in Sawhorse: Plugins > Install from .zip..., and **Enable...** in the Plugins pane. Plugins > Hello box > Add a Cube puts the cube in; Ctrl+Z takes it out in one step ("Undone: Add a cube").

Everything is in metres, Z up, like the kernel. Show and read lengths with [`units`](#sawhorseunits).

## The manifest

`plugin.json`, at the top of the plugin folder. The app checks it before listing the plugin; `npm run plugin:check` checks it the same way (the rules: [`packages/plugin-api/src/manifest.ts`](../packages/plugin-api/src/manifest.ts); the schema for editors: [`manifest.schema.json`](../packages/plugin-api/manifest.schema.json)).

| Field | Required | What |
|---|---|---|
| `id` | yes | Reverse-DNS, lower case: `org.example.cabinet-generator`. Unique; installing a plugin with the same id replaces the old one. |
| `name` | yes | 1 to 64 characters, shown everywhere. |
| `version` | yes | Semantic version: `1.2.0`. |
| `api` | yes | `1`. |
| `description` | yes | Up to 500 characters (may be empty). Shown in the Plugins pane and the enable dialog. |
| `author`, `license` | yes | Shown in the pane. |
| `permissions` | yes | What it may do (below). An empty list is fine. |
| `net` | with `net` | The hosts it may reach: `"api.example.com"`, `"*.example.org"` (subdomains only). No scheme, port or path. |
| `contributes` | no | What it adds: `commands`, `panels`, `tools`, `menus`, `shortcuts` (below). Anything here needs the `ui` permission. |
| `entry` | no | The module the app loads. Default `main.js`. |
| `homepage` | no | An `https://` address. |

**Contributions** are declared up front so the enable dialog and the pane can say what a plugin adds, and the app refuses to register anything else:

```json
"contributes": {
  "commands": [{ "id": "generate", "title": "Generate Cabinet" }],
  "panels": [{ "id": "cabinet", "title": "Cabinet" }],
  "tools": [{ "id": "drill", "title": "Pins", "hint": "Hover a face, click to drill.", "vcb": "Count, pitch, dia", "icon": "M6 3h12v18H6z" }],
  "menus": [{ "command": "generate", "group": "Make" }],
  "shortcuts": [{ "command": "generate", "key": "Alt+Shift+C" }, { "tool": "drill", "key": "Alt+Shift+P" }]
}
```

- Ids are lower-case letters, digits and dashes, starting with a letter.
- A tool's `vcb` is the Value Control Box label while it is active (without one the box is read-only); `icon` is SVG path data in a 24 x 24 box, drawn with the app's stroke (no other markup is accepted).
- Commands appear in the Plugins menu under the plugin's name (with their shortcut); tools appear there too, and in the rail's plugin group.
- A shortcut needs Ctrl, Alt or Meta and one letter, digit or F1 to F12. Single keys belong to the built-in tools. The app's own Ctrl shortcuts (Z, Y, S, O, I, A, G) win.

## Permissions

Declared in `permissions`, listed in the dialog the person sees before enabling, and enforced by the app on every call, whatever the plugin's code says. A call without its permission fails with an error whose `code` is `"permission"`.

| Permission | Lets the plugin | API |
|---|---|---|
| `model.read` | read solids, groups and components, faces, the tree, the selection (and set it), materials, the cut list; hear `selectionChanged`, `documentChanged`, `documentOpened` | `model.*` reads, `model.select`, those events |
| `model.write` | change the model, one named undo step per batch, validated by the kernel | `batch.commit()` |
| `ui` | panels, commands, tools, menus, shortcuts, messages, prompts and confirms; hear `toolChanged` | `ui.*` |
| `storage` | a small store of its own: per app (256 KB) and inside the document (64 KB, saved in the `.shz`) | `storage.app`, `storage.document` |
| `file` | save and open files, only through the app's save and open dialogs, where the person picks the file | `file.save`, `file.open` |
| `net` | requests to the hosts in `net`, over https, without cookies or credentials, through the app | `net.fetch`, and a global `fetch` |

Without `net` there is no `fetch` in the sandbox at all, and no other way out (see [What a plugin cannot do](#what-a-plugin-cannot-do)).

**A grant is for the files the person enabled**, not for the plugin's id. The app keeps a SHA-256 of the package (every file, its path and contents) with the grant. A different package with the same id (an update, a replacement zip, a zip that takes a bundled sample's id) is listed **Off**, with "These are not the files you enabled: enable it again to let it run", until the person enables it again and sees what it asks for; the dialog says when it takes a bundled sample's place. When a new version also asks for more than was granted, the message lists what is new. Two sources keep their grant when their files change, because the person chose the source rather than a build of it: a bundled sample updated with the app, and a folder loaded with Load unpacked folder on the desktop (Reload reads it again while you work on it).

## Lifecycle

1. **Listed.** On start the app lists the bundled samples and the installed plugins (desktop: `<app data>/plugins/<id>/`; browser: kept in this browser). A plugin whose manifest or code breaks the rules is listed with its errors and cannot start.
2. **Enabled.** The person clicks Enable..., reads the permissions and confirms. The grant is remembered for these files; the plugin starts now and on every later start, until its files change (above).
3. **Started.** The app makes a Web Worker for it (under the page's content security policy), locks it down, sends its modules, checks and links them and imports the entry. Top-level code runs (top-level `await` is fine); register commands, panels and tools there. An entry may also export `activate(sawhorse)`, which runs after the module loads. When the entry finishes, the plugin is **Running**.
4. **Running.** The app calls the plugin's handlers (a command, a panel button, a tool event) and delivers the events it listens to. Each call is answered when the handler (or the promise it returns) finishes.
5. **Not responding.** The app pings every plugin each second. One that has not answered for 5 s (busy in a loop) is marked Not responding, with Stop in the status line and the pane; after 15 s it is stopped. It runs in its own thread, so the app never freezes.
6. **Stopped.** Disable, Stop, Reload, a crash, the watchdog, or the app closing ends the worker. Its panels, commands and tools disappear at once. Reload starts it again (an unpacked folder is read again first).

Errors thrown by a handler are caught: they go to the developer console and the failing call, and the plugin keeps running. An error while the entry loads leaves it **Did not start**, with the message as its last error.

## The API

```js
import sawhorse, { model, ui, units, geometry, events, storage, file, net, log, plugin, apiVersion } from "sawhorse";
```

The types, with every call's permission, are in [`packages/plugin-api/index.d.ts`](../packages/plugin-api/index.d.ts). Add `// @ts-check` to your files and include that file in your editor's TypeScript setup to get completion and checking.

Calls that talk to the app return promises. `units` and `geometry` are local and synchronous. Values crossing to the app must be plain data: numbers, strings, booleans, `null`, lists, plain objects and `Uint8Array` (fields that are `undefined` are left out).

**Errors.** A call that talks to the app never throws for a refusal: its promise **rejects with a `PluginApiError`** (import it from `"sawhorse"`), whose `code` says why: `"permission"` (plugin.json does not ask for what the call needs), `"invalid"` (a parameter the app cannot use, including one that is not plain data: it is refused before anything is sent), `"too_large"` (the call, or the app's answer to it, is over the [message budget](#the-message-budget)), `"not_found"`, `"busy"` (too many calls at once or too fast: try again later), `"stopped"` (the plugin was stopped while the call waited), `"unknown"` (no such call), `"send_failed"` (the message could not be sent at all, for instance after the plugin was stopped) and `"refused"` (anything else the app said no to). Building a batch step by step (`model.batch(...)`, `b.op(...)`) is local and throws at the line with the mistake; `commit()` rejects.

```js
import { storage, PluginApiError } from "sawhorse";
try {
  await storage.app.set("sizes", sizes);
} catch (e) {
  if (e instanceof PluginApiError && e.code === "permission") ui.notify("Turn on storage for this plugin to remember sizes.");
  else throw e;
}
```

### The message budget

Every call, answer, callback and event is one message, serialized once by its sender into a single buffer that is handed to the other side without a copy. The receiver measures the buffer before it reads anything in it, then reads the values out of it with every limit below applied before anything is allocated for them. The app never relies on the sandbox's own checks (a plugin can replace any built-in in its worker), so the limits hold whatever a plugin does to its own globals. ADR-0009 has the design; [docs/LIMITS.md](LIMITS.md#plugins) lists every number with its reason.

| What | Budget |
|---|---|
| One message | 4 MB. `file.save` and the answer to `file.open`: 34 MB (a 32 MB file). `net.fetch`: 12 MB (a 10 MB body or answer). A plugin's modules at start: 24 MB in all. |
| Values | strings up to 4 M characters, byte arrays up to 32 MB (a `Uint8Array` that views part of a larger buffer sends only its own bytes; one over a `SharedArrayBuffer` or a resizable buffer is refused), 1,000,000 values, nested 64 deep, object keys up to 256 characters, no `__proto__` key, lists without holes or extra properties. |
| Rate | 300 messages a second (bursts of 100); above that requests are refused with `"busy"`, and ten seconds' worth in one second stops the plugin. The time the app spends on a plugin's messages is measured too: more than a quarter of every second for five seconds stops it. |
| Waiting at once | 64 requests; 4 batches; 4 `net.fetch`; one `file.save` or `file.open`; one `ui.prompt` or `ui.confirm`. Stopping a plugin drops its queued batches, closes its dialog and cancels its downloads. |

A call over the budget rejects with `code: "too_large"` and sends nothing; an answer the app cannot fit in the budget (a `model.solids()` of a huge model, say) rejects the call the same way, so your promise always settles. An event too large to deliver is dropped with a line in your console.

### sawhorse.model

All need `model.read`. Every read waits for edits already on their way, so it sees the model as it is after them.

```js
const doc = await model.document(); // { name, units, canUndo, canRedo, hash, context }
const stats = await model.stats(); // { solids, groups, components, instances, faces, edges, vertices, sketches, meshes, volume, watertight }
const solids = await model.solids(); // [{ id, number, name, label, tag, material, parent, min, max, volume, visible, locked, editable, inComponent }]
const one = await model.solid(solids[0].id); // or null
const faces = await model.faces(one.id); // [{ id, normal, origin, centroid, area }] by persistent face id
const nodes = await model.nodes(); // groups and component instances: { id, kind, name, label, definition, parent, tag, hidden, locked, visible, transform, inComponent }
const defs = await model.definitions(); // components: { id, name, instances, solids, min, max } (size in its own axes)
const tree = await model.tree(); // { kind: "model", children: [{ kind: "instance", id, label, open?, children }, ...] }
const sel = await model.selection(); // [{ kind: "solid", id } | { kind: "instance", id } | { kind: "face", solid, face } | ...]
await model.select([{ kind: "instance", id: nodes[0].id }]);
const box = await model.bounds(); // { min, max } round everything, or round given items; null when empty
const mats = await model.materials(); // [{ name, color: [r, g, b, a], texture, ... }]
const cut = await model.cutList({ rounding: "sixteenth", visibleOnly: false }); // { rows: [{ name, qty, thickness, width, length, material, grain, sheet, boardFeet, area, shaped, handed, hidden }], totals, hash }
```

`editable` says whether an edit may change a solid now: it is directly in the open group or component and not locked. A solid inside a closed group or component is not editable until the person opens it (double-click), exactly as for the built-in tools.

### Changing the model: batches

Every change is a **batch**: collect ops, then `commit()`. The app applies the whole batch through the kernel as **one undo step named after the batch** ("Undone: Generate cabinet"), validated like any edit. If any op is refused, nothing changes and `commit()` rejects with the reason, naming the op ("op 3 (push_pull): no face #12 on this object"). `commit()` needs `model.write`.

```js
const b = model.batch("Make a shelf");
const rect = b.op({ op: "add_rectangle", sketch: 1, x: 0, y: 0, w: 0.8, h: 0.3 }); // sketch 1 is the ground
const shelf = b.op({ op: "extrude", sketch: 1, region: rect.region(0), distance: 0.019 });
b.rename({ object: shelf.object }, "Shelf");
const comp = b.makeComponent([{ object: shelf.object }], "Shelf");
const result = await b.commit();
const instance = result.value(comp.node); // the new component instance's id
```

**Placeholders.** An op does not know the ids earlier ops of the same batch will make. Each `b.op(...)` returns a reference; `ref.get("path")` is a placeholder for the value at `path` in that op's outcome, filled in when the batch is applied:

| Shortcut | Path | For |
|---|---|---|
| `ref.object` | `object` (`objects.0` for an import or `box`) | the solid `extrude`, `box` or an import made |
| `ref.node` | `instance`, `group`, `groups.0` | the component instance or group `makeComponent`, `group` or `component` made |
| `ref.sketch` | `sketch` | the sketch `new_sketch` made or `draw` drew into |
| `ref.face` | `inside` (draw), `face` (imprint) | the face a closed stroke drawn on a face made |
| `ref.region(i)` | `regions.i` | a region a sketch edit made |

A placeholder may only name an earlier op. After `commit()`, `result.value(placeholder)` and `result.outcomes` give the real values.

**Helpers** build the ops for common things:

```js
b.box({ min: [0, 0, 0], size: [0.6, 0.3, 0.019], name: "Top", material: "Oak", tag: "Carcass" }); // ref.object
b.component({
  name: "Bench",
  instanceName: "Bench 1",
  at: [1, 0, 0], // or a transform
  parts: [
    { name: "Leg", boxes: [{ min: [0, 0, 0], size: [0.05, 0.05, 0.45] }], copies: [{ at: [0, 0, 0], name: "Front left leg" }, { at: [0.95, 0, 0], name: "Front right leg" }] },
    { name: "Top", boxes: [{ min: [0, 0, 0], size: [1, 0.35, 0.03] }], copies: [{ at: [0, 0, 0.45] }] },
  ],
}); // ref.node: the Bench instance; each part is a component, so the cut list lists Leg x 2 and Top
b.group([{ object: 7 }, { node: 3 }], "Carcass");
b.makeComponent([{ object: 7 }], "Side");
b.rename({ node: 3 }, "Left side");
b.delete({ node: 3 }); // or { object: 7 }
b.transform([{ node: 3 }], geometry.transform.translation([0, 0, 0.1]));
b.defineMaterial("Walnut", [92, 64, 51, 255]);
b.setMaterial(7, "Walnut");
b.setTag([{ object: 7 }], "Shelves");
```

### Ops a batch can hold

Any op of the kernel's op log ([`src/kernel/types.ts`](../src/kernel/types.ts) `KernelOp`, the same JSON the `.shz` history and the golden files use), except `undo`, `redo`, `batch`, `set_context`, `leave_context` and `restore`: history and navigation belong to the person. Lengths in metres, ids from the API or placeholders. The ones plugins reach for most:

| Op | Fields | Outcome |
|---|---|---|
| `new_sketch` | `origin`, `normal` | `{ sketch }` |
| `add_rectangle` | `sketch`, `x`, `y`, `w`, `h` (the sketch's own 2D frame) | `{ regions }` |
| `draw` | `target`: `{ on: "plane", origin, normal }`, `{ on: "face", object, face }` or `{ on: "sketch", sketch }`; `points` (world); `closed` | `{ sketch, regions, faces, inside }` |
| `extrude` | `sketch`, `region`, `distance` | `{ object }` |
| `push_pull`, `push_pull_new_face` | `object`, `face`, `distance` (negative pushes in, through the far side cuts through) | |
| `import` | `items` (polygon soups), `definitions`, `nodes` (components and groups), `materials` | `{ objects, meshes, groups, sketches, notes }` |
| `transform`, `copy`, `array` | `objects`, `nodes`, `xform` (and `count`) | `{ objects, nodes }` |
| `make_group`, `make_component`, `make_unique`, `explode` | `items` / `node`, `name` | `{ group }`, `{ instance, definition }` ... |
| `rename`, `rename_definition` | `item` / `definition`, `name` | |
| `delete_object`, `delete_node` | `object` / `node` | |
| `set_hidden`, `set_locked`, `tag_items`, `create_tag` | `items`, ... | |
| `define_material`, `set_material` | `name`, `color` / `object`, `material` | |
| `set_grain`, `set_stock` | `part`, `grain` / `stock`, `sheet` | (the cut list) |
| `offset`, `follow_me`, `imprint`, `split_face`, `erase_face` ... | as the built-in tools send them | |

A batch holds at most 2,000 ops, and at most 10 batches a second are applied. A batch also has a work budget: each op costs about the size of the model it is applied to, so ops times things in the model (solids, meshes, groups, instances, sketches) must stay under 20,000,000; split bigger work into several batches (each its own undo step). The batch as sent is at most 4 MB (about 150,000 points of geometry).

### sawhorse.ui

All need `ui`.

```js
await ui.registerCommand({ id: "generate", run: () => generate() }); // declared in contributes.commands
await ui.notify("Made a cabinet.", { kind: "info" }); // "warning", "error": the status line, prefixed with your plugin's name
const name = await ui.prompt({ title: "Name it", label: "Name", value: "Shelf" }); // string, or null when cancelled
const ok = await ui.confirm({ title: "Replace it?", text: "The selected cabinet is replaced.", ok: "Replace" }); // boolean
await ui.activateTool("drill"); // one of this plugin's tools
```

Dialogs are the app's, titled with the plugin's name ("from Cabinet generator"), so a plugin cannot pass itself off as the app.

### Panels

A panel is declarative: a list of fields the app draws with its own components, in the Plugins pane under the plugin's name (a collapsible section). No HTML from a plugin reaches the page.

```js
const panel = await ui.registerPanel({
  id: "cabinet", // declared in contributes.panels
  body: [
    { type: "number", id: "width", label: "Width", unit: "length", value: 0.6, min: 0 }, // metres, shown and typed in the document's units
    { type: "number", id: "shelves", label: "Shelves", unit: "count", value: 2 },
    { type: "text", id: "name", label: "Name", value: "Base" },
    { type: "select", id: "wood", label: "Wood", value: "oak", options: [{ value: "oak", label: "Oak" }, { value: "ash", label: "Ash" }] },
    { type: "checkbox", id: "back", label: "Back panel", value: true },
    { type: "table", id: "parts", columns: ["Part", "Qty"], rows: [["Side", 2]], align: ["left", "right"] },
    { type: "note", id: "status", text: "Ready.", tone: "info" }, // "warning", "error"
    { type: "image", id: "pic", src: "data:image/png;base64,...", alt: "Diagram", width: 120 },
    { type: "button", id: "generate", label: "Generate", primary: true },
  ],
  onChange(id, values) {}, // a field changed: number and text on Enter or when they lose focus
  onAction(id, values) {}, // a button was pressed
});
await panel.update({ fields: { status: { text: "Done.", tone: "info" }, generate: { label: "Update", disabled: false } }, values: { shelves: 3 } });
await panel.setBody([...]); // replace everything
```

`values` holds every input field by id. A typed length that cannot be read ("3 1/2 ft-ish") shows its error under the field and is not sent (the field keeps the focus to be fixed; Esc puts the old value back). Enter that commits a number or text field hands the keyboard back to the model, so the person's next tool key ("r", "p") picks a tool; a checkbox or a button keeps only Space and Enter. Images are base64 `data:` URLs of png, jpeg, gif or webp (SVG is refused: its size is not in its header), at most 256 KB each and 2048 x 2048 pixels (read from the image's header before the browser decodes it). One panel holds at most 100 fields, 5,000 table cells, 256 K characters of text and 4 images (512 KB in all); a body or an update over that is refused with `"invalid"`. Registering the same panel, command or tool again with nothing changed redraws nothing.

### Tools

A plugin tool is a tool like the built-in ones. It receives the same pointer events, already carrying the inference engine's answer (`snap`: the point, its kind, the face under it with its plane) and what is directly under the cursor (`picked`), and the Value Control Box's entries.

```js
await ui.registerTool({
  id: "drill", // declared in contributes.tools (title, hint, vcb, icon there)
  onActivate(tool) {},
  onDeactivate(tool) {},
  async onEvent(e, tool) {
    // e.type: "move" | "down" | "click" | "dblclick" | "drag" | "dragend" | "leave"
    // e.snap: { point, kind: "endpoint" | "on-face" | ..., tip, surface: { at: { kind: "face", solid, face }, frame: { origin, normal, u, v }, point }, entity }
    // e.picked, e.ray, e.x, e.y, e.shift, e.ctrl, e.alt
    await tool.setView({
      overlay: [{ kind: "loop", points: [...], color: "accent" }], // "segments" | "strip" | "loop"; colours: accent, preview, warning, ok, red, green, blue, muted, or an inference kind
      labels: [{ at: e.snap.point, text: "8 holes" }],
      status: "Click to drill.", // the status line (null: the tool's hint)
      value: "8, 32mm, 5mm", // shown in the Value Control Box
      busy: true, // a gesture is going on: Esc calls onCancel instead of clearing the selection
      inference: { start: [0, 0, 0], lock: { dir: [0, 0, 1], kind: "axis-z" }, plane: null, grid: true }, // like a built-in tool's constraints
    });
  },
  onValue(text, tool) {}, // Enter in the Value Control Box
  onCancel(tool) {}, // Esc while busy
});
```

The inference marker under the cursor is drawn for you. Pointer moves are coalesced: while your handler is still busy with one move, only the latest next one is kept; presses, clicks and values arrive in order. Change the model from a tool with a batch, as anywhere.

### sawhorse.units

No permission; local. Lengths in the document's display units (they follow the person's choice as it changes).

```js
units.system(); // "frac-in" | "in" | "ft-in" | "mm" | "cm" | "m"
units.short(); // "in", "mm" ...
units.parse("3/4"); // 0.01905 (a typed unit always wins: "19mm", "1'6\""); null when it cannot be read
units.read("nope"); // { ok: false, error: "Can't read \"nope\" as a length." }
units.format(0.2858); // "11 1/4", or "~ 285.8" when rounded
```

### sawhorse.geometry

No permission; pure functions. `Vec3` is `[x, y, z]`; a transform is `{ linear: [[...], [...], [...]], translation }` (row by row, as ops take it).

```js
const { vec3, plane, bbox, transform } = geometry;
vec3.add(a, b); vec3.sub(a, b); vec3.scale(a, s); vec3.dot(a, b); vec3.cross(a, b); vec3.length(a); vec3.normalize(a); vec3.distance(a, b); vec3.lerp(a, b, t); vec3.perpendicular(a);
const pl = plane.fromPointNormal(origin, normal); plane.distance(pl, p); plane.project(pl, p); plane.frame(pl); plane.intersectLine(pl, p, dir);
const b = bbox.fromPoints(points); bbox.union(a, b); bbox.size(b); bbox.center(b); bbox.contains(b, p); bbox.corners(b);
const m = transform.compose(transform.translation([0, 0, 1]), transform.rotation([0, 0, 1], 90, pivot));
transform.apply(m, p); transform.applyVector(m, v); transform.invert(m); transform.scale(2, pivot);
```

### sawhorse.events

```js
const off = events.on("documentChanged", ({ hash, canUndo }) => refresh()); // model.read; after every edit, undo, redo (coalesced)
events.on("selectionChanged", ({ selection }) => {}); // model.read
events.on("documentOpened", ({ name }) => {}); // model.read; New or Open
events.on("toolChanged", ({ tool }) => {}); // ui
events.on("unitsChanged", ({ units }) => {}); // no permission
off(); // stop listening
await off.ready; // resolves when the app has accepted the subscription
```

`on()` returns at once; its `ready` promise rejects with a `PluginApiError` when the app refuses the subscription (`"permission"`: `selectionChanged` without `model.read`, say) or the name is not an event (`"invalid"`). A refused subscription keeps no listener and is also written to the plugin's console.

### sawhorse.storage

Needs `storage`. JSON values by key; each plugin sees only its own.

```js
await storage.app.set("sizes", { width: 0.6 }); // kept by this app on this device (256 KB per plugin)
const sizes = await storage.app.get("sizes"); // null when missing
await storage.document.set("cabinet", 12); // kept with the open document, saved in its .shz (64 KB per plugin), cleared by New
await storage.document.keys(); await storage.document.delete("cabinet");
```

Any string of 1 to 200 characters is a key, including `constructor`, `toString` and `__proto__`. Values are JSON values (no `Uint8Array`: encode bytes as base64 text). All plugins together keep at most 1 MB in this browser and 1 MB in a document; a `set` that would pass either is refused with `"invalid"`. Sizes are measured without turning the value into text first, so an oversized value is refused at once.

### sawhorse.file

Needs `file`. Only through the app's dialogs: the person picks where a file goes and which file is opened.

```js
const saved = await file.save({ name: "Bench report.md", data: markdownText, mime: "text/markdown", description: "Markdown" }); // { name } or null
const picked = await file.open({ extensions: [".csv"], description: "CSV" }); // { name, bytes: Uint8Array } or null
```

### sawhorse.net

Needs `net` and only reaches the hosts listed in the manifest's `net`, over https (http only for `localhost`), with no cookies or credentials and no redirects. The app makes the request; the plugin gets the answer (at most 10 MB, 30 s). An answer that says it is larger (`Content-Length`) is refused before it is read, and one that turns out larger is cut off at 10 MB and refused (`"too_large"`). At most 4 requests wait at once and 50 MB a minute come in per plugin (`"busy"` past that).

```js
const res = await net.fetch("https://api.example.com/prices?wood=oak"); // a Response
const prices = await res.json();
// With "net", the global fetch does the same: await fetch("https://api.example.com/...")
```

The desktop app's content security policy does not yet let the window reach outside hosts, so `net` works in the browser build today (see ADR-0009).

### log and the developer console

`log(...)` and `console.log / info / warn / error` write to the plugin's developer console (Plugins pane, **Developer: console**; also the browser's console). The last error shows in the pane as the plugin's last error (its first 600 characters). At most 100 lines a second are kept, each cut to 4,000 characters as sent and 2,000 as kept.

## What a plugin cannot do

Enforced by the app and the sandbox, not by convention. Three locks, each enough on its own for what it covers:

1. **The code is checked before it runs.** Every module is parsed as an ES module (with [acorn](https://github.com/acornjs/acorn), the way the browser will read it) by the app's loader, again by the sandbox, and by `npm run plugin:check`; the rules are read from the syntax tree, so no arrangement of comments, strings or regular expressions hides anything. A module the parser cannot read is refused.
2. **The worker's global object is cut down to an allowlist** before the plugin's code loads ([below](#what-a-plugins-global-object-has)): anything not on it, including every browser API added after this was written, is gone.
3. **The worker runs under the page's content security policy.** It is started from a `blob:` URL, so the browser applies the app's policy to it (the browser build's is in its `index.html`, the desktop app's in `src-tauri/tauri.conf.json`): no script or font from anywhere but the app itself.

So a plugin cannot:

- **Reach the page.** No DOM, no `window`, no `document`; the app draws panels and tool overlays from data.
- **Reach the network** without `net`: no `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`, `WebTransport`, `FontFace` (a font load is a request to any address) or `Notification`; no `import()` of anything.
- **Load code from elsewhere.** A module may import only `"sawhorse"` and its own files by a relative path (`./lib/parts.js`). `import(...)` (and any other `import.<something>` but `import.meta`) and `importScripts` are refused before the plugin runs; `importScripts` and `URL.createObjectURL` are removed from the worker too, and the policy refuses outside and `data:` scripts.
- **Turn text into code.** `eval`, `new Function(...)` (and its async and generator forms) and string timers throw.
- **Keep data elsewhere.** IndexedDB, Cache Storage, the origin private file system, the older sandboxed file systems (`webkitRequestFileSystem` and its synchronous form), storage buckets and Web Locks are removed; use `storage`.
- **Talk to other pages or start workers.** `BroadcastChannel`, `MessageChannel`, `Worker`, `SharedWorker` and raw `postMessage` are removed.
- **Use devices.** `navigator` keeps only facts about the browser (`language`, `languages`, `hardwareConcurrency`, `onLine`, `platform`, `userAgent` and the like); USB, HID, serial, Bluetooth, permissions and the GPU are not there.
- **Run on someone else's grant.** A grant belongs to the files the person enabled ([Permissions](#permissions)).
- **Freeze the app.** It runs in its own thread; the watchdog stops it when it stops answering, and the app stops it when it sends too much, too fast, or keeps the app busy ([The message budget](#the-message-budget)). **Stop** in the Plugins pane ends it at any time.
- **Exhaust the app's memory through messages.** Every message is measured before it is read, against the budget above; nothing a plugin claims (a list's length, a buffer's size) is allocated before the bytes show it is really there.
- **Change history or navigate.** Undo, redo, opening and closing groups and replacing the document are the person's.

### What a plugin's global object has

The language's own built-ins (`Object`, `Array`, `Map`, `Promise`, `Math`, `JSON`, `Intl`, typed arrays, `WeakRef` ...) and `WebAssembly`, plus: `console`; `setTimeout`, `setInterval` (functions only), `queueMicrotask`, `requestAnimationFrame`, `structuredClone`; `TextEncoder`, `TextDecoder`, `atob`, `btoa`, `URL`, `URLSearchParams`, `Blob`, `File`, `FileReader`; streams and `CompressionStream`; `Headers`, `Request`, `Response` (bodies only: `net.fetch` answers with a `Response`); `crypto` (`getRandomValues`, `randomUUID`, `subtle`) and `performance`; `OffscreenCanvas`, `ImageData`, `Path2D`, `createImageBitmap` and the `DOMMatrix` family (to draw a picture for a panel's image field); `Event`, `EventTarget`, `AbortController`, `DOMException`; `self`, `location` and `navigator` (cut down). The full list is `ALLOWED_GLOBALS` in [`src/plugins/lockdown.ts`](../src/plugins/lockdown.ts). TypeScript's `WebWorker` library declares more than this; what it declares beyond the list is `undefined` at run time.

## Checking, packing, installing

In this repository:

```bash
npm run plugin:check                         # every plugin under plugins/: manifest, sources, type check against index.d.ts
npm run plugin:check -- path/to/my-plugin    # one folder anywhere: the same, with its // @ts-check files type-checked
npm run plugin:pack -- plugins/cabinet-generator   # -> dist-plugins/com.sawhorse3d.cabinet-generator-1.0.0.zip
npm run plugin:pack -- my-plugin --out my-plugin.zip
```

Under `plugins/` (the samples' rule) a missing `README.md` or `LICENSE` fails the check; anywhere else it is a warning. The type check covers the files that start with `// @ts-check`; a file without it is named in a warning and not checked. For a folder outside this repository the check uses **your project's own `tsconfig.json`** when there is one (in the plugin's folder or above it, up to the folder with your `package.json`; the report says "with <path>"), so your compiler options hold; without one it uses the samples' options (strict). Paths are printed relative to where you ran the command, or in full when the plugin is elsewhere. `plugin:pack` checks first and writes the same bytes for the same folder (files in a fixed order with a fixed date); without `--out` it writes into this repository's `dist-plugins/` (ignored by git), so pass `--out` to put the zip somewhere else.

**Installing** (Plugins menu, or the buttons under the list in the Plugins pane):

| | Desktop | Browser |
|---|---|---|
| Install from .zip... | unpacked to `<app data>/plugins/<id>/`, kept | kept in this browser |
| Load unpacked folder... | read from the folder you pick; **Reload** reads it again (for working on a plugin) | - |
| Load from .zip for this session... | - | until the tab closes |
| Open folder | shows the folder in your file manager | - |
| **Uninstall** (an installed plugin) | removes the folder | removes it from this browser |
| **Remove** (an unpacked folder or a session plugin) | takes it off the list; the folder stays | takes it off the list |

A zip may hold the folder's contents or the folder itself. Installing a zip whose `id` is already listed replaces that plugin, and the new files are off until they are enabled ([Permissions](#permissions)). The bundled samples are always listed; they cannot be uninstalled, only disabled, and one that a zip with its id replaced comes back when that plugin is uninstalled or removed.

**Working on a plugin**: on the desktop, Load unpacked folder, then edit and click Reload (the folder keeps its grant while you edit it). In the browser, pack and Load from .zip for this session (each new zip is enabled again). Watch Developer: console.

## Publishing

There is no plugin directory yet. Share the zip (a release on your repository works well) and its README, say what each permission is for, and keep the `id` the same across versions so an install replaces the old one (each new version is enabled again by the person who installs it: the grant was for the old files). A later version of Sawhorse may add a directory, signing and update checks; the manifest and API 1 will keep working (`api` stays the contract: a plugin says which API it speaks, and the app refuses one it does not).

`packages/plugin-api` (types, schema, shim) is meant to be published to npm as `sawhorse-plugin-api` for editor support; until it is, copy `index.d.ts` and `manifest.schema.json` from this repository.
