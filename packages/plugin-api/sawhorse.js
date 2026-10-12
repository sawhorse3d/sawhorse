// The "sawhorse" module, as a plugin imports it: `import { model, ui } from "sawhorse"`.
//
// A tiny shim. The plugin's sandbox (one Web Worker per plugin) builds the API before the
// plugin's entry module loads and leaves it under a well-known symbol; this module hands
// it out. Outside a Sawhorse plugin sandbox there is nothing to hand out, so it says so.
// Types: index.d.ts beside this file.
const api = globalThis[Symbol.for("sawhorse.plugin-api")];
if (!api) throw new Error('The "sawhorse" module only works inside a Sawhorse plugin (it is provided by the app).');

export const apiVersion = api.apiVersion;
export const plugin = api.plugin;
export const log = api.log;
export const model = api.model;
export const ui = api.ui;
export const units = api.units;
export const geometry = api.geometry;
export const events = api.events;
export const storage = api.storage;
export const file = api.file;
export const net = api.net;
export default api;
