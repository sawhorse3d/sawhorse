// Orbit and Pan as tools (lane 30, issue #36). A MacBook's trackpad is a mouse with no
// middle button, so the middle-drag orbit was out of reach: two-finger click and drag pans
// (the right button) and a pinch zooms, but nothing orbited.
//
// The tools themselves do nothing with the pointer: the camera controller
// (src/viewport/controls.ts) takes a left press before the tool sees it whenever
// `leftButtonNav` says so, and runs the very drag a middle press runs (the same pivot under
// the cursor, the marker, the clamps, the look-at point). They exist so the left button has
// a mode to be in (the O and H keys, the rail, the View menu) and so the status line says
// what a drag does. Esc goes back to the tool that was active before (App.tsx).
//
// Alt (Option on a Mac) with the left button orbits, and Alt+Shift pans, in every tool
// whose `keepsAlt()` is false: all of them but Paint (Alt+click picks a material up) and
// Section Plane (Alt reverses the cut). A plugin's tool is one of the others.
//
// On macOS, Control+click is the right click (src/lib/platform.ts), and a right-drag pans in
// every tool, so a left press with Control pans in every tool too, Shift or Alt or not.
import type { NavMode } from "../viewport/controls";
import type { ToolId } from "../lib/tools";
import { Tool, type ToolView } from "./tool";

/** What a left-button press does to the view under `tool`: "orbit" or "pan" takes it from
 * the tool (the Orbit and Pan tools, Alt where Alt is free, or the Mac's Control+click), null
 * leaves it to the tool. `macControl`: the press is a Control+click on macOS. */
export function leftButtonNav(tool: Tool | null, mods: { alt: boolean; shift: boolean; macControl?: boolean }): Exclude<NavMode, "pinch"> | null {
  if (mods.macControl) return "pan";
  const own = tool?.navigation() ?? null;
  if (own === "pan") return "pan";
  if (own === "orbit") return mods.shift ? "pan" : "orbit";
  if (mods.alt && !(tool?.keepsAlt() ?? false)) return mods.shift ? "pan" : "orbit";
  return null;
}

/** The Orbit and Pan tools' ids. */
export function isNavTool(id: ToolId): id is "orbit" | "pan" {
  return id === "orbit" || id === "pan";
}

abstract class NavTool extends Tool {
  view(): ToolView {
    return { overlay: [], marker: null, labels: [], box: null, cursor: "grab" };
  }
}

export class OrbitTool extends NavTool {
  readonly id = "orbit" as const;
  navigation(): "orbit" {
    return "orbit";
  }
}

export class PanTool extends NavTool {
  readonly id = "pan" as const;
  navigation(): "pan" {
    return "pan";
  }
}
