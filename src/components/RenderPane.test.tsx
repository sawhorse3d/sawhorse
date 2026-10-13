import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RenderPane } from "./RenderPane";
import { useCredits } from "../credits/useCredits";
import { ServiceError, ServiceUnreachableError, type Balance, type RenderRequest, type RenderServiceClient } from "../credits/client";
import { NothingToRenderError, type RenderPasses } from "../render/passes";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const PASSES: RenderPasses = {
  width: 1024,
  height: 640,
  depthPng: new Uint8Array([137, 80, 78, 71, 1]),
  edgesPng: new Uint8Array([137, 80, 78, 71, 2]),
  clayPng: new Uint8Array([137, 80, 78, 71, 3]),
  depthPreview: "data:image/png;base64,AAAA",
  modelPixels: 4000,
};

class FakeClient {
  baseUrl = "http://svc.test";
  credits: number;
  up = true;
  renders: RenderRequest[] = [];
  renderOutcome: "ok" | "failed" | "broke" | "limited" = "ok";
  checkouts: string[] = [];
  claims: { step: string; email: string; code?: string }[] = [];
  constructor(credits: number) {
    this.credits = credits;
  }
  private bal(): Balance {
    return { install_id: "abcdefghijklmnopqrst", credits: this.credits, devices: 1, max_devices: 2, render_cost: 1, packs: [{ id: "p100", credits: 100, amount: 1000, currency: "usd", label: "$10" }, { id: "p250", credits: 250, amount: 2500, currency: "usd", label: "$25" }, { id: "p500", credits: 500, amount: 5000, currency: "usd", label: "$50" }] };
  }
  async balance() {
    if (!this.up) throw new ServiceUnreachableError("Render service not connected (http://svc.test).");
    return this.bal();
  }
  async render(req: RenderRequest) {
    if (!this.up) throw new ServiceUnreachableError("down");
    this.renders.push(req);
    if (this.renderOutcome === "failed") throw new ServiceError(502, "render_failed", "failed", { balance: this.credits });
    if (this.renderOutcome === "limited") throw new ServiceError(429, "rate_limited", "Limit reached: 6 renders a minute. Try again in 40 s.", { retry_after: 40 });
    if (this.credits < 1 || this.renderOutcome === "broke") throw new ServiceError(402, "insufficient_credits", "No credits", { balance: 0 });
    this.credits--;
    return { request_id: req.request_id, status: "succeeded" as const, provider: "mock", mock: true, width: 1024, height: 640, image_url: "http://svc.test/v1/renders/x/image?exp=1&sig=s", expires_at: "", balance: this.credits };
  }
  async history() {
    return { renders: [{ request_id: "r1", status: "succeeded" as const, prompt: "Oak bookshelf", preset: "Workshop", provider: "mock", created_at: "", image_url: "http://svc.test/h1.png" }, { request_id: "r2", status: "refunded" as const, prompt: "x", preset: null, provider: null, created_at: "" }] };
  }
  async checkout(pack: string) {
    this.checkouts.push(pack);
    return { session_id: "cs_1", url: "http://svc.test/dev/checkout/cs_1", mode: "simulated", credits: 100 };
  }
  async claimRequest(email: string) {
    this.claims.push({ step: "request", email });
    return { sent: true, message: "If that address bought credits, a code is on its way." };
  }
  async claimRedeem(email: string, code: string) {
    this.claims.push({ step: "redeem", email, code });
    this.credits = 230;
    return { ok: true, credits: 230, merged: 30, unlinked_devices: 0, devices: 2 };
  }
}

function Harness({ client, capture }: { client: FakeClient; capture: () => Promise<RenderPasses> }) {
  const credits = useCredits(client as unknown as RenderServiceClient);
  return (
    <>
      <b data-testid="chip">{credits.state.connection === "connected" ? credits.state.credits : "—"}</b>
      <RenderPane credits={credits} capture={capture} />
    </>
  );
}

async function mount(client: FakeClient, capture: () => Promise<RenderPasses> = async () => PASSES) {
  render(<Harness client={client} capture={capture} />);
  await waitFor(() => expect(document.querySelector(".note.svc")?.getAttribute("data-connection")).not.toBe("connecting"));
}

describe("Render pane", () => {
  it("renders through the service: sends the passes, shows the image, takes one credit", async () => {
    const client = new FakeClient(3);
    await mount(client);
    expect(screen.getByTestId("chip").textContent).toBe("3");
    expect(screen.getByText(/Render service connected · 3 credits/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Kitchen/ }));
    fireEvent.click(screen.getByRole("button", { name: "Render · 1 credit" }));
    await waitFor(() => expect(document.getElementById("result")?.getAttribute("data-state")).toBe("done"));
    const req = client.renders[0];
    expect(req.prompt).toMatch(/warm kitchen/);
    expect(req.preset).toBe("Kitchen");
    expect(atob(req.depth_png).charCodeAt(4)).toBe(1);
    expect(atob(req.edges_png!).charCodeAt(4)).toBe(2);
    expect(atob(req.clay_png!).charCodeAt(4)).toBe(3);
    expect(req.request_id).toMatch(/^[A-Za-z0-9_-]{22}$/);
    // Lane 15: fetched into a blob: URL when it can be; here (no such host) the address itself.
    await waitFor(() => expect((document.getElementById("renderImg") as HTMLImageElement | null)?.src).toBe("http://svc.test/v1/renders/x/image?exp=1&sig=s"));
    expect(screen.getByText("Mock render · no provider keys")).toBeTruthy();
    expect(screen.getByTestId("chip").textContent).toBe("2");
  });

  it("says the service is not connected (lane-1 behaviour) and charges nothing", async () => {
    const client = new FakeClient(5);
    client.up = false;
    await mount(client);
    expect(screen.getByTestId("chip").textContent).toBe("—");
    expect(screen.getByText("Render service not connected.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Render · 1 credit/ }));
    expect(document.getElementById("result")?.textContent).toMatch(/Render service not connected \(http:\/\/svc\.test\)/);
    fireEvent.click(screen.getByRole("button", { name: /\$25/ }));
    expect(client.checkouts).toEqual([]);
    expect(client.renders).toEqual([]);
  });

  it("asks for a pack when the balance is empty, without calling the service", async () => {
    const client = new FakeClient(0);
    await mount(client);
    fireEvent.click(screen.getByRole("button", { name: /Render · 1 credit/ }));
    expect(document.getElementById("result")?.textContent).toBe("No credits on this device. Buy a pack below.");
    expect(client.renders).toHaveLength(0);
  });

  it("explains failures: refunded render, rate limit, nothing in view", async () => {
    const client = new FakeClient(4);
    client.renderOutcome = "failed";
    await mount(client);
    fireEvent.click(screen.getByRole("button", { name: /Render · 1 credit/ }));
    await waitFor(() => expect(document.getElementById("result")?.textContent).toMatch(/failed and your credit was returned/));
    expect(screen.getByTestId("chip").textContent).toBe("4");
    client.renderOutcome = "limited";
    fireEvent.click(screen.getByRole("button", { name: /Render · 1 credit/ }));
    await waitFor(() => expect(document.getElementById("result")?.textContent).toMatch(/6 renders a minute/));
    cleanup();
    await mount(new FakeClient(4), async () => {
      throw new NothingToRenderError("Draw something first: the render follows the model in the current view.");
    });
    fireEvent.click(screen.getByRole("button", { name: /Render · 1 credit/ }));
    await waitFor(() => expect(document.getElementById("result")?.textContent).toMatch(/Draw something first/));
  });

  it("opens checkout in a new window from the click and watches for the credits", async () => {
    const client = new FakeClient(0);
    const fakeWin = { closed: false, opener: {}, location: { href: "" }, close: vi.fn() };
    const open = vi.spyOn(window, "open").mockReturnValue(fakeWin as unknown as Window);
    await mount(client);
    fireEvent.click(screen.getByRole("button", { name: /\$10/ }));
    await waitFor(() => expect(document.getElementById("result")?.textContent).toMatch(/simulated checkout/i));
    expect(open).toHaveBeenCalledWith("", "_blank");
    expect(fakeWin.opener).toBeNull();
    expect(fakeWin.location.href).toBe("http://svc.test/dev/checkout/cs_1");
    expect(client.checkouts).toEqual(["p100"]);
    expect(screen.getByText(/waiting for checkout/)).toBeTruthy();
    // The payment lands; the next poll (focus here) shows it and stops waiting.
    client.credits = 100;
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(screen.getByTestId("chip").textContent).toBe("100"));
    expect(screen.queryByText(/waiting for checkout/)).toBeNull();
  });

  it("offers a link when the browser blocked the checkout window", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    await mount(new FakeClient(0));
    fireEvent.click(screen.getByRole("button", { name: /\$50/ }));
    await waitFor(() => expect(screen.getByRole("link", { name: "open checkout" }).getAttribute("href")).toBe("http://svc.test/dev/checkout/cs_1"));
  });

  it("shows the history gallery", async () => {
    await mount(new FakeClient(1));
    fireEvent.click(screen.getByRole("button", { name: "History" }));
    await waitFor(() => expect(screen.getByAltText("Oak bookshelf")).toBeTruthy());
    expect(screen.getByText("failed · refunded")).toBeTruthy();
    fireEvent.click(screen.getByAltText("Oak bookshelf").closest("button")!);
    await waitFor(() => expect((document.getElementById("renderImg") as HTMLImageElement | null)?.src).toBe("http://svc.test/h1.png"));
  });

  it("moves credits with an emailed claim code", async () => {
    const client = new FakeClient(30);
    await mount(client);
    fireEvent.click(screen.getByRole("button", { name: "Move credits" }));
    fireEvent.change(screen.getByLabelText("Email used at checkout"), { target: { value: "wood@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Email me a code" }));
    await waitFor(() => expect(screen.getByText(/a code is on its way/)).toBeTruthy());
    fireEvent.change(screen.getByLabelText("Claim code"), { target: { value: "ABCD-2345" } });
    fireEvent.click(screen.getByRole("button", { name: "Move credits here" }));
    await waitFor(() => expect(document.getElementById("result")?.textContent).toMatch(/Credits moved to this device: 230/));
    expect(client.claims).toEqual([
      { step: "request", email: "wood@example.com" },
      { step: "redeem", email: "wood@example.com", code: "ABCD-2345" },
    ]);
    expect(screen.getByTestId("chip").textContent).toBe("230");
  });
});
