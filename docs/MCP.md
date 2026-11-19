# The MCP server

`sawhorse-mcp` lets an AI agent build and read Sawhorse models ([#14](https://github.com/sawhorse3d/sawhorse/issues/14)). It is a [Model Context Protocol](https://modelcontextprotocol.io) server on the same kernel as the app: the agent starts it, talks to it over stdin and stdout, and the `.shz` files it saves open in the app as they are, with the same model hash.

It speaks MCP revision **2025-11-25** (a client asking for 2025-06-18 gets that back, since its tool calls are the same here; one asking for any other gets 2025-11-25). Messages are JSON-RPC 2.0, one a line. It has tools only: no resources, prompts or sampling. Nothing goes to the network, no window opens and nothing asks a question.

## Running it

It is built from a checkout for now, like `sawhorse-cli`; the installers do not include it. In the checkout's `crates/` folder:

```bash
cargo build --release -p sawhorse-mcp
```

The binary is `target/release/sawhorse-mcp` (`.exe` on Windows). Start it with the one folder it may use:

```bash
sawhorse-mcp ~/furniture
```

An MCP client starts it itself. For Claude Code, say:

```bash
claude mcp add sawhorse -- /path/to/sawhorse-mcp /path/to/furniture
```

Other clients take the same command and argument in their server list.

## What it holds to

- **One folder.** It reads and writes only `.shz` files under the folder it was started with. Paths are relative to it, like `Bookshelf.shz` or `jobs/Bench.shz`. An absolute path, a path with `..`, or one that leads out through a link (a linked folder, or a linked file) is refused. Folders are not made for you. A name that would mean something else on Windows is refused on every system, so a path names the same file everywhere: one holding `:`, `\` (folders are separated by `/`), a control character or one of `< > " | ? *`, one ending in a dot or a space, and a device name such as `CON`, `NUL`, `AUX`, `COM1` or `LPT1` before its first dot.
- **Where it was tried.** CI runs every test of the server on Linux, Windows and macOS on every change, and on Windows and macOS also builds the release binary and runs it once on a fresh folder (a board added, saved and its cut list read, over stdin and stdout). On Windows the same rules come from the same code (a drive, a `\\server\share` path or a `..\` gives an absolute path or a `..`, which are refused), and the tests there try drive, drive-relative, rooted, UNC, `\\?\` and `\\.\` paths, `..` with `\` and mixed separators, junctions and links into and out of the folder, streams and device names in any case, names ending in a dot, a name differing only in case from a file that is there, a path over 260 characters, a start folder given as a junction, a relative path, with a separator at its end or by its 8.3 short name, and a save over a file that is read-only or that another program holds open. On macOS they try a start folder behind a link (the system's temporary folder is one) and a volume that ignores case. Reported on [#14](https://github.com/sawhorse3d/sawhorse/issues/14) by @aramshaw on 2026-11-18: a Windows binary built by CI, run on Windows 11 from a real MCP client and from a folder on a Samba share, reached by a mapped drive and by its UNC path, gave the worked example's model hashes, saved over a file there with no temporary file left, and refused the paths out of the folder. He found one gap there, [#118](https://github.com/sawhorse3d/sawhorse/issues/118): a first save through a link that the file server itself follows could land outside the folder, while opening that path afterwards was refused. A save now checks where the file it wrote really is (below), so that first save is refused too. Not tried: a real MCP client on macOS.
- **The check and the write are two steps.** A path is checked, then the file is opened or written. Another program that can make links inside the folder while a save runs could point it elsewhere in between. The server itself never makes a link.
- **Saving.** `save_model` writes through the kernel's own writer, the file the app writes. It writes a new temporary file beside the target (`.<name>.<process>-<n>.sawhorse-mcp-tmp`, never one that was there), checks where that file really is, reads it back and checks its model hash, and only then moves it over the target. A temporary file that resolves outside the folder is removed and the save refused with the words for any path out of the folder: through a link the file server follows (a Samba share seen from Windows), a new file's folder resolves where it was asked and only a file made in it shows where it really is ([#118](https://github.com/sawhorse3d/sawhorse/issues/118)). So so a save that fails leaves the old file as it was and removes only its own temporary file. A link or a hard link at the target is replaced by the new file, never written through. It keeps no `.bak` copy, as the desktop app does. A save over a read-only file replaces it on Linux and macOS; on Windows it is refused ("Access is denied") and the old file is kept. On Windows a save over a file another program holds open is done when that program lets others delete the file, and refused otherwise; either way the file there is whole, the old one or the new one, and no temporary file is left. A name differing only in case from a file that is there names that file on a volume that ignores case, as Windows and macOS volumes do by default: it opens that file, and a save under it leaves one file (on Windows it took the name the save gave, on macOS it kept the name it had).
- **All or nothing.** Every tool that changes the model is one undo step, worked out on a copy and kept only when the whole change validates. A refusal says why and changes nothing.
- **What changed.** Each change answers with the cut list rows it added, removed and changed (`changes`), and the new `model_hash`. A row counts as changed when its name, quantity, size (to 0.1 mm), material, grain or items move; a change only to what is hidden, the stock or what is counted shows in `cut_list` and the hash, not in `changes`.
- **Names.** Parts are reported by name, never by a geometry index; a change names a part by the ids `summary` gives. Where two parts share a name, `summary` lists it under `shared_names`, and `add_board` lists the others under `shares_name_with`. Neither picks one.
- **Untrusted input.** A message that is not JSON, nested deeper than the parser allows (128 levels), longer than 4 MiB, or of the wrong shape gets a JSON-RPC error back, and the server reads on. A longer message is read past without being held.

## Tools

Lengths in tool answers are millimetres, to 0.1 mm, where the field says `_mm` (3/4 in reads 19.0). The `cut_list` and `nest` answers and `apply_ops`' ops are the kernel's own, in metres, exact.

| Tool | Takes | Does |
|---|---|---|
| `new_model` | `units`: `mm` or `in` (optional) | Starts a new empty model, dropping the one held. `units` is the unit system saved with the file. |
| `open_model` | `path` | Opens a `.shz`, replacing the model held. Answers `file` (where it is on this machine, as the server sees it), its `model_hash`, its part count and the reader's `notes` (what it had to repair). |
| `save_model` | `path` (optional) | Saves the model. Without a path, where it was opened from or last saved. Answers `path`, and `file`: where it is on this machine, as the server sees it (under WSL, a `/mnt/c/...` path; on Windows the long `\\?\C:\...` form the system gives for a resolved path, with `\` between folders). |
| `summary` | nothing | Every part (one per cut list row): `name`, `qty`, `size_mm` (thickness, width, length), `material`, `grain`, `target` and `items` (the ids `apply_ops` names). Also `path`, `unsaved`, `model_hash`, `display_units`, `materials` and `shared_names`. |
| `add_board` | `name`, `thickness`, `width`, `length`; optional `position` (the corner nearest the origin, default `[0, 0, 0]`), `length_axis` (`x`, `y` or `z`, default `x`), `thickness_axis` (default `z`: lying flat), `units` (`mm`, the default, `cm`, `m` or `in`), `material` | Adds one solid board, named. `material` is one in the model, or one from the app's material library by name (Pine, Red oak, Walnut, Baltic birch ply, MDF and the rest), which is added with it. Answers `board` (its `object` id, `min_mm` and `max_mm`), `changes` and `shares_name_with`. |
| `apply_ops` | `name` (names the undo step), `ops` | Applies kernel ops as one step: the named batch a plugin sends ([PLUGINS.md](PLUGINS.md)), with `{"$ref": i, "path": "..."}` placeholders for what an earlier op made. To repeat parts, `copy` or `array` them by the ids `summary` gives rather than adding each again. Undo, redo, batches, restores and opening a group are not allowed, and a batch holds at most 20,000 points in all (a sketch's edge limit). Every array of two or three numbers in the ops counts as a point, a plane's origin and normal included. Answers each op's `outcome` and `changes`; a refusal names the op and why. |
| `cut_list` | `rounding` (`sixteenth`, `thirty_second` or `half_mm`), `visible_only` | The cut list, the same JSON as `sawhorse-cli cutlist` prints for the saved file, with the keys of each object in alphabetical order. |
| `nest` | `algorithm` (`guillotine` or `maxrects`), `visible_only` | The nesting, the same JSON as `sawhorse-cli nest`, keys in alphabetical order as for `cut_list`. |
| `undo`, `redo` | nothing | Undo or redo the last change. Answers `changes`. |

A tool that fails answers with `isError: true` and an `error` saying why. An unknown tool, or arguments that are not an object, is a JSON-RPC error (`-32602`).

## A worked example

A shelf of two 19 mm pine sides and two shelves, saved. Each line the agent sends is one message; the answers are shortened here.

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"agent","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"new_model","arguments":{"units":"mm"}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_board","arguments":{"name":"Side","thickness":19,"width":250,"length":600,"length_axis":"z","thickness_axis":"x","material":"Pine"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"add_board","arguments":{"name":"Side","thickness":19,"width":250,"length":600,"position":[481,0,0],"length_axis":"z","thickness_axis":"x","material":"Pine"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"add_board","arguments":{"name":"Shelf","thickness":19,"width":250,"length":462,"position":[19,0,0],"material":"Pine"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"add_board","arguments":{"name":"Shelf","thickness":19,"width":250,"length":462,"position":[19,0,581],"material":"Pine"}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"summary","arguments":{}}}
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"save_model","arguments":{"path":"Shelf.shz"}}}
```

The second side's answer says it shares its name, and what the cut list gained:

```json
{"board":{"object":2,"name":"Side","material":"Pine","min_mm":[481,0,0],"max_mm":[500,250,600]},
 "shares_name_with":[{"name":"Side","qty":1,"size_mm":[19,250,600],"material":"Pine","items":[{"object":1}]}],
 "changes":{"added":[{"name":"Side","qty":1,"size_mm":[19,250,600],"material":"Pine","items":[{"object":2}]}],"removed":[],"changed":[]},
 "model_hash":"..."}
```

The summary lists four parts and `"shared_names":[{"name":"Shelf","parts":2},{"name":"Side","parts":2}]`. To make the bottom shelf an inch deeper, the agent changes it by its object id from `items` (3) and the box `add_board` answered (`min_mm` `[19,0,0]`, `max_mm` `[481,250,19]`): it moves the four corners of the shelf's back face, at 250 mm, 25.4 mm further back. No tool names a face, so a push/pull, which takes a face id, is not the way here.

```json
{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"apply_ops","arguments":{"name":"Deeper shelf","ops":[{"op":"transform_geometry","object":3,"points":[[0.019,0.25,0],[0.481,0.25,0],[0.019,0.25,0.019],[0.481,0.25,0.019]],"xform":{"linear":[[1,0,0],[0,1,0],[0,0,1]],"translation":[0,0.0254,0]},"autofold":false}]}}}
```

The answer's `changes.changed` shows that row's size before and after (`size_mm` `[19,250,462]`, then `[19,275.4,462]`). After another `save_model`, `Shelf.shz` opens in the app as it is, and `sawhorse-cli cutlist Shelf.shz` prints the cut list the server showed.

## Not yet

- `check_clearance` (the gap or overlap between two parts) and `measure` (the distance between two parts' faces): the next step on #14.
- Named dimensions ("make the carcass 900 wide and run it again"): they need parameters in the model, a file format question. For now an agent keeps its own spec and rebuilds.
- Driving the app's window, rendering drawings, or the installers.
