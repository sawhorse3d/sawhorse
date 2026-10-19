import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TopBar, type MenuItem } from "./TopBar";

afterEach(cleanup);

// A submenu entry (Plugins > Everything > Ask Me): pointing at it opens it, and a click on it
// must leave it open. It used to toggle, so point-then-click closed the submenu the pointer
// had just opened; on a slow CI runner plugins-ui.spec.ts lost "Ask Me" that way (twice).
function bar(onAsk = vi.fn()) {
  const menus: Partial<Record<string, MenuItem[]>> = {
    Plugins: [
      { label: "Everything", items: [{ label: "Ask Me", keys: "Alt+Shift+K", onSelect: onAsk }] },
      { label: "Other", items: [{ label: "Hello", onSelect: () => {} }] },
      "-",
      { label: "Manage Plugins…", onSelect: () => {} },
    ],
  };
  render(<TopBar view={null} onView={() => {}} credits={null} menus={menus} />);
  fireEvent.click(screen.getByRole("button", { name: "Plugins" }));
  return { onAsk, entry: (name: string) => screen.getByRole("menuitem", { name }) };
}

const askMe = () => screen.queryByRole("menuitem", { name: /Ask Me/ });

describe("TopBar submenus", () => {
  it("point, then click: the submenu stays open", () => {
    const { entry } = bar();
    fireEvent.mouseEnter(entry("Everything").parentElement!);
    expect(askMe()).not.toBeNull();
    expect(entry("Everything").getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(entry("Everything"));
    expect(askMe()).not.toBeNull();
    fireEvent.click(entry("Everything"));
    expect(askMe()).not.toBeNull();
  });

  it("a click without pointing first (keyboard, touch) opens it", () => {
    const { entry } = bar();
    expect(askMe()).toBeNull();
    fireEvent.click(entry("Everything"));
    expect(askMe()).not.toBeNull();
  });

  it("another entry closes it; choosing a command runs it and closes the menu", () => {
    const { entry, onAsk } = bar();
    fireEvent.click(entry("Everything"));
    fireEvent.mouseEnter(entry("Other").parentElement!);
    expect(askMe()).toBeNull();
    fireEvent.mouseEnter(entry("Manage Plugins…"));
    expect(screen.queryByRole("menuitem", { name: "Hello" })).toBeNull();
    fireEvent.click(entry("Everything"));
    fireEvent.click(askMe()!);
    expect(onAsk).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu", { name: "Plugins" })).toBeNull();
  });
});
