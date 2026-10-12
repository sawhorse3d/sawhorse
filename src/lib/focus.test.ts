import { describe, expect, it } from "vitest";
import { isEditableText, keyBelongsToControl } from "./focus";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Record<string, string> = {}): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

describe("isEditableText: who keeps Ctrl+Z", () => {
  it("text fields keep it", () => {
    expect(isEditableText(el("input"))).toBe(true);
    expect(isEditableText(el("input", { type: "text" }))).toBe(true);
    expect(isEditableText(el("input", { type: "number" }))).toBe(true);
    expect(isEditableText(el("input", { type: "search" }))).toBe(true);
    expect(isEditableText(el("textarea"))).toBe(true);
    const div = el("div");
    div.contentEditable = "true";
    // jsdom does not compute isContentEditable; a browser does.
    Object.defineProperty(div, "isContentEditable", { value: true });
    expect(isEditableText(div)).toBe(true);
  });

  it("pane controls do not: the app undoes", () => {
    expect(isEditableText(el("select"))).toBe(false);
    expect(isEditableText(el("input", { type: "checkbox" }))).toBe(false);
    expect(isEditableText(el("input", { type: "radio" }))).toBe(false);
    expect(isEditableText(el("input", { type: "color" }))).toBe(false);
    expect(isEditableText(el("input", { type: "range" }))).toBe(false);
    expect(isEditableText(el("button"))).toBe(false);
    expect(isEditableText(el("tr"))).toBe(false);
    expect(isEditableText(null)).toBe(false);
    expect(isEditableText(window)).toBe(false);
    expect(isEditableText(el("input", { type: "text", readonly: "" }))).toBe(false);
  });
});

describe("keyBelongsToControl: who gets a tool key", () => {
  it("text fields and selects keep every key", () => {
    for (const k of ["r", "p", "Escape", " ", "Delete"]) {
      expect(keyBelongsToControl(el("input"), k), k).toBe(true);
      expect(keyBelongsToControl(el("input", { type: "number" }), k), k).toBe(true);
      expect(keyBelongsToControl(el("textarea"), k), k).toBe(true);
      expect(keyBelongsToControl(el("select"), k), k).toBe(true);
    }
  });

  // The verifier: after using a plugin panel's control, "r" and "p" did nothing until the
  // focus was moved off it.
  it("a checkbox, radio, range or colour picker keeps only the keys it uses", () => {
    for (const type of ["checkbox", "radio", "range", "color"]) {
      for (const k of ["r", "p", "1", "Escape", "Delete"]) expect(keyBelongsToControl(el("input", { type }), k), `${type} ${k}`).toBe(false);
      for (const k of [" ", "Enter", "ArrowLeft", "ArrowDown"]) expect(keyBelongsToControl(el("input", { type }), k), `${type} ${k}`).toBe(true);
    }
    expect(keyBelongsToControl(el("button"), "r")).toBe(false);
    expect(keyBelongsToControl(null, "r")).toBe(false);
  });
});
