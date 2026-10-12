# sawhorse-plugin-api

Everything a Sawhorse plugin is written against, plugin API version 1:

| File | What |
|---|---|
| `index.d.ts` | The types of the `"sawhorse"` module a plugin imports, with the permission each call needs |
| `sawhorse.js` | The runtime shim: inside a plugin's sandbox it hands out the API the app built |
| `manifest.schema.json` | JSON Schema for `plugin.json` (editors can validate against it with `"$schema"`) |
| `src/manifest.ts` | The manifest validator the app and `npm run plugin:check` use |
| `src/source.ts` | What plugin code may import and contain (each module parsed with acorn and checked before it runs) |
| `src/rpc.ts` | The message protocol between the app and a plugin's Web Worker |
| `src/runtime.ts` | The plugin side of the API (batches with placeholders, callbacks, events) |
| `src/geometry.ts` | `sawhorse.geometry`: vectors, planes, boxes, transforms |

The authoring guide is [`docs/PLUGINS.md`](../../docs/PLUGINS.md); the decision record is [ADR-0009](../../docs/adr/0009-plugin-api.md).

## Type-checking a plugin

Plugins are plain ES modules. Put `// @ts-check` at the top of each file and include `index.d.ts` in the check:

```json
{
  "compilerOptions": { "allowJs": true, "checkJs": true, "noEmit": true, "target": "ES2022", "module": "ESNext", "moduleResolution": "bundler", "lib": ["ES2022", "WebWorker"] },
  "include": ["*.js", "path/to/sawhorse-plugin-api/index.d.ts"]
}
```

In this repository, `npm run plugin:check` does that for every plugin under `plugins/` and validates their manifests; `npm run plugin:check -- path/to/plugin` does the same for a folder anywhere (its `// @ts-check` files). Note that a plugin's sandbox has fewer globals than TypeScript's `WebWorker` library declares (no `fetch` without `net`, no `FontFace`, no storage of its own): the list is in `docs/PLUGINS.md`, "What a plugin's global object has".

The package is not on npm yet (`"private": true`); copy `index.d.ts` and `manifest.schema.json`, or point at this folder. Licence: MIT OR Apache-2.0, like Sawhorse.
