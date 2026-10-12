import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { DEMO_BANNER_KEY, DemoBanner } from "./DemoBanner";
import { isWebDemo, RELEASES_URL, sitePage } from "../lib/site";

afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
});
beforeEach(() => localStorage.clear());

describe("demo banner", () => {
  it("says alpha, local, and links the desktop app", () => {
    render(<DemoBanner />);
    const note = screen.getByRole("note", { name: "About this demo" });
    expect(note.textContent).toContain("Alpha");
    expect(note.textContent).toContain("Runs in your browser, nothing is uploaded.");
    const link = screen.getByRole("link", { name: "Download the desktop app" });
    expect(link.getAttribute("href")).toBe(RELEASES_URL);
    expect(RELEASES_URL).toBe("https://github.com/sawhorse3d/sawhorse/releases/latest");
    expect(link.getAttribute("rel")).toContain("noopener");
  });

  it("dismisses and stays dismissed on this browser", () => {
    render(<DemoBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("note")).toBeNull();
    expect(localStorage.getItem(DEMO_BANNER_KEY)).toBe("1");
    cleanup();
    render(<DemoBanner />);
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("still dismisses when storage throws (private windows)", () => {
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    render(<DemoBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("note")).toBeNull();
    set.mockRestore();
    get.mockRestore();
  });
});

describe("site helpers", () => {
  it("is the demo only when the build says so", () => {
    vi.stubEnv("VITE_WEB_DEMO", "");
    expect(isWebDemo()).toBe(false);
    vi.stubEnv("VITE_WEB_DEMO", "1");
    expect(isWebDemo()).toBe(true);
  });

  it("links site pages beside the /app/ base, absolute elsewhere", () => {
    vi.stubEnv("BASE_URL", "/app/");
    expect(sitePage("guide/")).toBe("/guide/");
    vi.stubEnv("BASE_URL", "/sawhorse/app/");
    expect(sitePage("/guide/")).toBe("/sawhorse/guide/");
    vi.stubEnv("BASE_URL", "./");
    expect(sitePage("guide/")).toBe("https://sawhorse3d.com/guide/");
  });
});
