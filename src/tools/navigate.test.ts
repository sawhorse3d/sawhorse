// Lane 30 (issue #36): which left presses move the view instead of reaching the tool. A
// MacBook's trackpad has no middle button, so the Orbit and Pan tools take a plain left
// drag, and Alt (Option) + drag orbits in every tool that gives Alt no meaning of its own.
import { describe, expect, it } from "vitest";
import { fakeHost } from "./fakeHost";
import { isNavTool, leftButtonNav, OrbitTool, PanTool } from "./navigate";
import { SelectTool } from "./select";
import { MoveTool } from "./move";
import { LineTool } from "./line";
import { EraseTool } from "./erase";
import { PushTool } from "./push";
import { RotateTool } from "./rotate";
import { ScaleTool } from "./scale";
import { TapeTool } from "./tape";
import { PaintTool } from "./paint";
import { SectionTool } from "./section";
import { PluginTool } from "./plugin";
import type { Tool } from "./tool";

const none = { alt: false, shift: false };
const shift = { alt: false, shift: true };
const alt = { alt: true, shift: false };
const altShift = { alt: true, shift: true };

describe("leftButtonNav", () => {
  it("the Orbit tool orbits with a plain drag and pans with Shift, Alt or not", () => {
    const t = new OrbitTool(fakeHost());
    expect(leftButtonNav(t, none)).toBe("orbit");
    expect(leftButtonNav(t, alt)).toBe("orbit");
    expect(leftButtonNav(t, shift)).toBe("pan");
    expect(leftButtonNav(t, altShift)).toBe("pan");
  });

  it("the Pan tool pans whatever is held", () => {
    const t = new PanTool(fakeHost());
    for (const m of [none, shift, alt, altShift]) expect(leftButtonNav(t, m)).toBe("pan");
  });

  it("a tool without an Alt meaning keeps the plain left button, and Alt orbits (Alt+Shift pans)", () => {
    const h = fakeHost();
    const tools: Tool[] = [new SelectTool(h), new EraseTool(h), new LineTool(h), new PushTool(h), new MoveTool(h), new RotateTool(h), new ScaleTool(h), new TapeTool(h)];
    for (const t of tools) {
      expect(leftButtonNav(t, none)).toBeNull();
      expect(leftButtonNav(t, shift)).toBeNull();
      expect(leftButtonNav(t, alt)).toBe("orbit");
      expect(leftButtonNav(t, altShift)).toBe("pan");
    }
  });

  it("Paint and Section Plane keep Alt (picking a material up, reversing the cut)", () => {
    // Their behaviour is only `keepsAlt`; built from the prototype, they need no context.
    for (const t of [Object.create(PaintTool.prototype) as Tool, Object.create(SectionTool.prototype) as Tool]) {
      expect(t.keepsAlt()).toBe(true);
      for (const m of [none, shift, alt, altShift]) expect(leftButtonNav(t, m)).toBeNull();
    }
  });

  it("a plugin's tool is one without an Alt meaning, and so is no tool at all", () => {
    const t = Object.create(PluginTool.prototype) as Tool;
    expect(t.keepsAlt()).toBe(false);
    expect(leftButtonNav(t, none)).toBeNull();
    expect(leftButtonNav(t, alt)).toBe("orbit");
    expect(leftButtonNav(t, altShift)).toBe("pan");
    expect(leftButtonNav(null, none)).toBeNull();
    expect(leftButtonNav(null, alt)).toBe("orbit");
  });

  it("on macOS a Control+click is the right click: it pans in every tool, Shift or Alt or not", () => {
    const h = fakeHost();
    const tools: (Tool | null)[] = [
      new SelectTool(h),
      new MoveTool(h),
      new TapeTool(h),
      new OrbitTool(h),
      new PanTool(h),
      Object.create(PaintTool.prototype) as Tool,
      Object.create(SectionTool.prototype) as Tool,
      Object.create(PluginTool.prototype) as Tool,
      null,
    ];
    for (const t of tools) for (const m of [none, shift, alt, altShift]) expect(leftButtonNav(t, { ...m, macControl: true })).toBe("pan");
  });

  it("the Orbit and Pan tools ask for the grab cursor and are the navigation tools", () => {
    expect(new OrbitTool(fakeHost()).view().cursor).toBe("grab");
    expect(new PanTool(fakeHost()).view().cursor).toBe("grab");
    expect(isNavTool("orbit")).toBe(true);
    expect(isNavTool("pan")).toBe(true);
    expect(isNavTool("select")).toBe(false);
    expect(isNavTool("plugin:x/y")).toBe(false);
  });
});
