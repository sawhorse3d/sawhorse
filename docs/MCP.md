# The MCP server

`sawhorse-mcp` lets an AI agent build and read Sawhorse models ([#14](https://github.com/sawhorse3d/sawhorse/issues/14)). It is a [Model Context Protocol](https://modelcontextprotocol.io) server on the same kernel as the app: the agent starts it, talks to it over stdin and stdout, and the `.shz` files it saves open in the app as they are, with the same model hash.

It speaks MCP revision **2025-11-25** (a client asking for 2025-06-18 or 2025-03-26 gets that back; their tool calls are the same here). Messages are JSON-RPC 2.0, one a line. It has tools only: no resources, prompts or sampling. Nothing goes to the network, no window opens and nothing asks a question.

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

- **One folder.** It reads and writes only `.shz` files under the folder it was started with. Paths are relative to it, like `Bookshelf.shz` or `jobs/Bench.shz`. An absolute path, a path with `..`, or one that leads out through a link (a linked folder, or a linked file) is refused. Folders are not made for you.
- **Saving.** `save_model` writes through the kernel's own writer, the file the app writes. It writes beside the target, reads that back and checks its model hash, and only then puts it in place, so a save that fails leaves the old file as it was.
- **All or nothing.** Every tool that changes the model is one undo step, worked out on a copy and kept only when the whole change validates. A refusal says why and changes nothing.
- **What changed.** Each change answers with the cut list rows it added, removed and changed (`changes`), and the new `model_hash`.
- **Names.** Parts are found by name, never by a geometry index. Where two parts share a name, `summary` lists it under `shared_names`, and `add_board` lists the others under `shares_name_with`. Neither picks one.
- **Untrusted input.** A message that is not JSON, nested deeper than the parser allows (128 levels), longer than 16 MiB, or of the wrong shape gets a JSON-RPC error back, and the server reads on. A longer message is read past without being held.

## Tools

Lengths in tool answers are millimetres where the field says `_mm`. The `cut_list` and `nest` answers and `apply_ops`' ops are the kernel's own, in metres.

| Tool | Takes | Does |
|---|---|---|
| `new_model` | `units`: `mm` or `in` (optional) | Starts a new empty model, dropping the one held. `units` is the unit system saved with the file. |
| `open_model` | `path` | Opens a `.shz`, replacing the model held. Answers its `model_hash`, its part count and the reader's `notes` (what it had to repair). |
| `save_model` | `path` (optional) | Saves the model. Without a path, where it was opened from or last saved. |
| `summary` | nothing | Every part (one per cut list row): `name`, `qty`, `size_mm` (thickness, width, length), `material`, `grain`, `target` and `items` (the ids `apply_ops` names). Also `path`, `unsaved`, `model_hash`, `display_units`, `materials` and `shared_names`. |
| `add_board` | `name`, `thickness`, `width`, `length`; optional `position` (the corner nearest the origin, default `[0, 0, 0]`), `length_axis` (`x`, `y` or `z`, default `x`), `thickness_axis` (default `z`: lying flat), `units` (`mm`, the default, `cm`, `m` or `in`), `material` | Adds one solid board, named. `material` is one in the model, or one from the app's material library by name (Pine, Red oak, Walnut, Baltic birch ply, MDF and the rest), which is added with it. Answers `board` (its `object` id, `min_mm` and `max_mm`), `changes` and `shares_name_with`. |
| `apply_ops` | `name` (names the undo step), `ops` | Applies kernel ops as one step: the named batch a plugin sends ([PLUGINS.md](PLUGINS.md)), with `{"$ref": i, "path": "..."}` placeholders for what an earlier op made. Undo, redo, batches and restores are not allowed. Answers each op's `outcome` and `changes`; a refusal names the op and why. |
| `cut_list` | `rounding` (`sixteenth`, `thirty_second` or `half_mm`), `visible_only` | The cut list, the same JSON as `sawhorse-cli cutlist`. |
| `nest` | `algorithm` (`guillotine` or `maxrects`), `visible_only` | The nesting, the same JSON as `sawhorse-cli nest`. |
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

The summary lists four parts and `"shared_names":[{"name":"Shelf","parts":2},{"name":"Side","parts":2}]`. To make the bottom shelf an inch deeper, the agent changes it by its object id from `items`, for example with a push/pull through `apply_ops`, and the answer's `changes.changed` shows that row's size before and after. `Shelf.shz` then opens in the app as it is, and `sawhorse-cli cutlist Shelf.shz` prints the cut list the server showed.

## Not yet

- `check_clearance` (the gap or overlap between two parts) and `measure` (the distance between two parts' faces): the next step on #14.
- Named dimensions ("make the carcass 900 wide and run it again"): they need parameters in the model, a file format question. For now an agent keeps its own spec and rebuilds.
- Driving the app's window, rendering drawings, or the installers.
- MCP revision 2026-07-28 (stateless requests): the server keeps one model between calls, which 2025-11-25 suits.
