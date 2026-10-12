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
  "$schema": "../../packages/plugin-api/manifest.schema.json",
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

Zip the folder (`npm run plugin:pack -- hello-box` in this repository, or any zip tool), then in Sawhorse: Plugins > Install from .zip..., and **Enable...** in the Plugins pane. Plugins > Hello box > Add a Cube puts the cube in; Ctrl+Z takes it out in one step ("Undone: Add a cube").

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

Without `net` there is no `fetch` in the sandbox at all. When a new version asks for more than was granted, the plugin is off until the person enables it again and sees the new list.

## Lifecycle

1. **Listed.** On start the app lists the bundled samples and the installed plugins (desktop: `<app data>/plugins/<id>/`; browser: kept in this browser). A plugin whose manifest or code breaks the rules is listed with its errors and cannot start.
2. **Enabled.** The person clicks Enable..., reads the permissions and confirms. The grant is remembered; the plugin starts now and on every later start.
3. **Started.** The app makes a Web Worker for it, locks it down, sends its modules, links them and imports the entry. Top-level code runs (top-level `await` is fine); register commands, panels and tools there. An entry may also export `activate(sawhorse)`, which runs after the module loads. When the entry finishes, the plugin is **Running**.
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

A batch holds at most 10,000 ops.

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

`values` holds every input field by id. A typed length that cannot be read ("3 1/2 ft-ish") shows its error under the field and is not sent. Images are `data:` URLs of png, jpeg, gif, webp or svg only.

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
```

### sawhorse.storage

Needs `storage`. JSON values by key; each plugin sees only its own.

```js
await storage.app.set("sizes", { width: 0.6 }); // kept by this app on this device (256 KB per plugin)
const sizes = await storage.app.get("sizes"); // null when missing
await storage.document.set("cabinet", 12); // kept with the open document, saved in its .shz (64 KB per plugin), cleared by New
await storage.document.keys(); await storage.document.delete("cabinet");
```

### sawhorse.file

Needs `file`. Only through the app's dialogs: the person picks where a file goes and which file is opened.

```js
const saved = await file.save({ name: "Bench report.md", data: markdownText, mime: "text/markdown", description: "Markdown" }); // { name } or null
const picked = await file.open({ extensions: [".csv"], description: "CSV" }); // { name, bytes: Uint8Array } or null
```

### sawhorse.net

Needs `net` and only reaches the hosts listed in the manifest's `net`, over https (http only for `localhost`), with no cookies or credentials and no redirects. The app makes the request; the plugin gets the answer (at most 10 MB, 30 s).

```js
const res = await net.fetch("https://api.example.com/prices?wood=oak"); // a Response
const prices = await res.json();
// With "net", the global fetch does the same: await fetch("https://api.example.com/...")
```

The desktop app's content security policy does not yet let the window reach outside hosts, so `net` works in the browser build today (see ADR-0009).

### log and the developer console

`log(...)` and `console.log / info / warn / error` write to the plugin's developer console (Plugins pane, **Developer: console**; also the browser's console). The last error shows in the pane as the plugin's last error. At most 100 lines a second are kept.

## What a plugin cannot do

Enforced by the app and the sandbox, not by convention:

- **Reach the page.** No DOM, no `window`, no `document`; the app draws panels and tool overlays from data.
- **Reach the network** without `net`: `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource` and `WebTransport` are removed from the worker.
- **Load code from elsewhere.** A module may import only `"sawhorse"` and its own files by a relative path (`./lib/parts.js`); `import()` and `importScripts` are refused before the plugin runs, and `importScripts` is removed from the worker too.
- **Turn text into code.** `eval`, `new Function(...)` (and its async and generator forms) and string timers throw.
- **Keep data elsewhere.** IndexedDB, Cache Storage and the origin private file system are removed; use `storage`.
- **Talk to other pages or start workers.** `BroadcastChannel`, `Worker` and `SharedWorker` are removed.
- **Freeze the app.** It runs in its own thread; the watchdog stops it when it stops answering.
- **Change history or navigate.** Undo, redo, opening and closing groups and replacing the document are the person's.

## Checking, packing, installing

In this repository:

```bash
npm run plugin:check                         # every plugin under plugins/: manifest, sources, type check against index.d.ts
npm run plugin:check -- path/to/my-plugin    # one folder anywhere (no README / LICENSE rule outside plugins/)
npm run plugin:pack -- plugins/cabinet-generator   # -> dist-plugins/com.sawhorse3d.cabinet-generator-1.0.0.zip
npm run plugin:pack -- my-plugin --out my-plugin.zip
```

`plugin:pack` checks first and writes the same bytes for the same folder (files in a fixed order with a fixed date).

**Installing** (Plugins menu, or the buttons under the list in the Plugins pane):

| | Desktop | Browser |
|---|---|---|
| Install from .zip... | unpacked to `<app data>/plugins/<id>/`, kept | kept in this browser |
| Load unpacked folder... | read from the folder you pick; **Reload** reads it again (for working on a plugin) | - |
| Load from .zip for this session... | - | until the tab closes |
| Open folder | shows the folder in your file manager | - |
| Uninstall | removes the folder | removes it from this browser |

A zip may hold the folder's contents or the folder itself. The bundled samples are always listed; they cannot be uninstalled, only disabled.

**Working on a plugin**: on the desktop, Load unpacked folder, then edit and click Reload. In the browser, pack and Load from .zip for this session. Watch Developer: console.

## Publishing

There is no plugin directory yet. Share the zip (a release on your repository works well) and its README, say what each permission is for, and keep the `id` the same across versions so an install replaces the old one. A later version of Sawhorse may add a directory, signing and update checks; the manifest and API 1 will keep working (`api` stays the contract: a plugin says which API it speaks, and the app refuses one it does not).

`packages/plugin-api` (types, schema, shim) is meant to be published to npm as `sawhorse-plugin-api` for editor support; until it is, copy `index.d.ts` and `manifest.schema.json` from this repository.
