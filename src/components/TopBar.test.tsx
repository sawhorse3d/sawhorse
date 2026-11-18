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

// Lane 67 (#97): a title whose menu has no entry did nothing on a click (Help in the desktop
// app, Draw and Window in every build). Now only titles with an entry are in the bar.
describe("TopBar titles", () => {
  const item = (label: string, onSelect = () => {}): MenuItem => ({ label, onSelect });
  const titles = () => screen.getAllByRole("button").filter((b) => b.closest("nav")).map((b) => b.textContent);
  const ALL = ["File", "Edit", "View", "Draw", "Tools", "Plugins", "Window", "Help"];

  it("a title with no entries is not in the bar; the others keep their order", () => {
    render(
      <TopBar
        view={null}
        onView={() => {}}
        credits={null}
        menus={{ Help: [item("User Guide")], Window: [], Plugins: [item("Manage Plugins…")], File: [item("New")], View: [item("Model")] }}
      />,
    );
    expect(titles()).toEqual(["File", "View", "Plugins", "Help"]);
    expect(screen.queryByRole("button", { name: "Window" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Draw" })).toBeNull();
  });

  it("every title shown opens its menu with its entries, says so to a screen reader, and closes on a second click or Esc", () => {
    const menus = Object.fromEntries(ALL.map((m) => [m, [item(`${m} entry`)]]));
    render(<TopBar view={null} onView={() => {}} credits={null} menus={menus} />);
    expect(titles()).toEqual(ALL);
    for (const m of ALL) {
      const title = screen.getByRole("button", { name: m });
      expect(title.getAttribute("aria-haspopup")).toBe("menu");
      expect(title.getAttribute("aria-expanded")).toBe("false");
      fireEvent.click(title);
      expect(title.getAttribute("aria-expanded")).toBe("true");
      const menu = screen.getByRole("menu", { name: m });
      expect(menu.textContent).toBe(`${m} entry`);
      fireEvent.click(title);
      expect(screen.queryByRole("menu", { name: m })).toBeNull();
      fireEvent.click(title);
      fireEvent.keyDown(window, { key: "Escape" });
      expect(screen.queryByRole("menu", { name: m })).toBeNull();
      expect(title.getAttribute("aria-expanded")).toBe("false");
    }
  });

  it("choosing an entry runs it and closes the menu", () => {
    const go = vi.fn();
    render(<TopBar view={null} onView={() => {}} credits={null} menus={{ Help: [item("User Guide", go)] }} />);
    fireEvent.click(screen.getByRole("button", { name: "Help" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "User Guide" }));
    expect(go).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu", { name: "Help" })).toBeNull();
  });

  it("a title whose last entry goes while its menu is open leaves the bar, and comes back closed", () => {
    const props = { view: null, onView: () => {}, credits: null };
    const { rerender } = render(<TopBar {...props} menus={{ File: [item("New")], Plugins: [item("Hello")] }} />);
    fireEvent.click(screen.getByRole("button", { name: "Plugins" }));
    expect(screen.getByRole("menu", { name: "Plugins" })).not.toBeNull();
    rerender(<TopBar {...props} menus={{ File: [item("New")], Plugins: [] }} />);
    expect(titles()).toEqual(["File"]);
    expect(screen.queryByRole("menu")).toBeNull();
    rerender(<TopBar {...props} menus={{ File: [item("New")], Plugins: [item("Hello")] }} />);
    expect(titles()).toEqual(["File", "Plugins"]);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.getByRole("button", { name: "Plugins" }).getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "File" }));
    expect(screen.getByRole("menu", { name: "File" }).textContent).toBe("New");
  });

  it("no menus at all: no titles, and nothing else in the bar changes", () => {
    render(<TopBar view={null} onView={() => {}} credits={3} />);
    expect(titles()).toEqual([]);
    expect(screen.getByRole("navigation", { name: "Menu" })).not.toBeNull();
    expect(screen.getByRole("button", { name: /Credits/ })).not.toBeNull();
  });
});
