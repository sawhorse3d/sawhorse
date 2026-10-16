// The Styles pane's lane-15 controls: soft and hidden edges shown dashed (a style switch),
// and Soften selected (an angle on the selected solids, one undo step).
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { softenTargetOf } from "../App";
import { StylesPane, type StylesPaneProps } from "./StylesPane";

function props(over: Partial<StylesPaneProps> = {}): StylesPaneProps {
  return { style: null, shadows: null, onStyle: vi.fn(), onShadows: vi.fn(), onPreviewShadows: vi.fn(), ...over };
}

afterEach(cleanup);

describe("Styles pane, soft edges (lane 15)", () => {
  it("the soft-edges switch is part of the style", () => {
    const p = props();
    render(<StylesPane {...p} />);
    fireEvent.click(screen.getByLabelText("Soft and hidden edges"));
    expect(p.onStyle).toHaveBeenLastCalledWith({ edges: { soft: true } });
  });

  it("Soften selected sends the angle and the two choices; off without a selection", () => {
    const onSoften = vi.fn();
    const { rerender } = render(<StylesPane {...props({ onSoften, softenTarget: null })} />);
    const button = screen.getByRole("button", { name: "Soften selected" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(screen.getByTestId("soften").textContent).toContain("Select solids");
    rerender(<StylesPane {...props({ onSoften, softenTarget: { objects: [4], label: "1 solid selected" } })} />);
    expect(button.disabled).toBe(false);
    fireEvent.change(screen.getByLabelText("Soften angle"), { target: { value: "30" } });
    fireEvent.click(screen.getByLabelText("Smooth"));
    fireEvent.click(button);
    expect(onSoften).toHaveBeenLastCalledWith(30, true, false);
    expect(screen.getByTestId("soften").textContent).toContain("1 solid selected");
  });

  it("the target is every solid the selection names", () => {
    expect(softenTargetOf([])).toBeNull();
    expect(
      softenTargetOf([
        { kind: "object", object: 2 },
        { kind: "face", object: 2, face: 5 },
        { kind: "edge", owner: { kind: "object", id: 7 }, a: [0, 0, 0], b: [1, 0, 0], drawn: false },
        { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true },
      ]),
    ).toEqual({ objects: [2, 7], label: "2 solids selected" });
  });
});

describe("Styles pane sliders (lane 15)", () => {
  it("a run of arrow-key steps on a slider is one undo step", () => {
    vi.useFakeTimers();
    try {
      const sun = { on: true, month: 6, day: 21, minutes: 13 * 60 + 30, latitude: 45, light: 80, dark: 45, ground: true, faces: true };
      const p = props({ shadows: sun });
      const { rerender } = render(<StylesPane {...p} />);
      const s = screen.getByLabelText("Latitude");
      for (const v of [46, 47, 48]) {
        fireEvent.change(s, { target: { value: String(v) } });
        fireEvent.keyUp(s, { key: "ArrowRight" });
        vi.advanceTimersByTime(200);
      }
      expect(p.onShadows).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1000);
      expect(p.onShadows).toHaveBeenCalledTimes(1);
      expect(p.onShadows).toHaveBeenLastCalledWith(expect.objectContaining({ latitude: 48 }));
      // The document now says 48; a pointer release with nothing moved is not an edit.
      rerender(<StylesPane {...p} shadows={{ ...sun, latitude: 48 }} />);
      fireEvent.pointerUp(s);
      expect(p.onShadows).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("Styles pane, the ground grid (lane 21, issue #4)", () => {
  it("is on by default and its checkbox switches it through onGrid", () => {
    const onGrid = vi.fn();
    const p = props({ onGrid });
    const { rerender } = render(<StylesPane {...p} />);
    const box = screen.getByLabelText("Ground grid") as HTMLInputElement;
    expect(box.checked).toBe(true);
    fireEvent.click(box);
    expect(onGrid).toHaveBeenLastCalledWith(false);
    expect(p.onStyle).not.toHaveBeenCalled();
    rerender(<StylesPane {...p} style={{ grid: false }} />);
    expect((screen.getByLabelText("Ground grid") as HTMLInputElement).checked).toBe(false);
    // Hiding the grid is not a look: the default preset still reads as chosen.
    expect(screen.getByRole("button", { name: /Sawhorse/ }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByLabelText("Ground grid"));
    expect(onGrid).toHaveBeenLastCalledWith(true);
  });

  it("without onGrid it commits the style, keeping the preset", () => {
    const p = props({ style: { preset: "Clay", face: "monochrome" } });
    render(<StylesPane {...p} />);
    fireEvent.click(screen.getByLabelText("Ground grid"));
    expect(p.onStyle).toHaveBeenLastCalledWith({ preset: "Clay", face: "monochrome", grid: false });
  });

  it("choosing a preset keeps the grid as it is", () => {
    const p = props({ style: { grid: false } });
    render(<StylesPane {...p} />);
    fireEvent.click(screen.getByRole("button", { name: /X-ray/ }));
    expect(p.onStyle).toHaveBeenLastCalledWith({ preset: "X-ray", face: "x_ray", grid: false });
  });
});
