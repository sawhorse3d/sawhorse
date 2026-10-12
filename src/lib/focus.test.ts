import { describe, expect, it } from "vitest";
import { isEditableText } from "./focus";

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
