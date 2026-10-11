import { useEffect, useRef, useState } from "react";
import { newRequestId, PACKS, ServiceError, ServiceUnreachableError, type HistoryItem, type Pack } from "../credits/client";
import { storageNote } from "../credits/device";
import type { CreditsHandle } from "../credits/useCredits";
import { NothingToRenderError, passesToRequest, type RenderPasses } from "../render/passes";

// The Render pane from the approved mock, wired to the render service (ADR-0004): presets,
// prompt, Render (1 credit), the result, a history gallery, credit packs that open Stripe
// Checkout in the system browser, and "Move credits" (claim code) for a new device.

export const PRESETS: { name: string; sub: string; prompt: string }[] = [
  { name: "Workshop", sub: "soft daylight, oak", prompt: "Oak bookshelf in a bright workshop, soft daylight through a window, sawdust on the bench" },
  { name: "Kitchen", sub: "painted, warm", prompt: "Painted bookshelf in a warm kitchen, morning light, ceramic bowls on the shelves" },
  { name: "Living room", sub: "walnut, evening", prompt: "Walnut bookshelf in a living room at evening, lamp light, books and a plant" },
  { name: "Exterior", sub: "deck, overcast", prompt: "Cedar shelf unit on a covered deck, overcast sky, wet boards" },
];

export const IDLE_TEXT = "Render appears here. The model's depth and edge passes are sent with the prompt; nothing else leaves this machine.";

type Result =
  | { kind: "idle" }
  | { kind: "info" | "error"; text: string; link?: { href: string; label: string } }
  | { kind: "rendering"; preview: string; started: number; stage: string }
  | { kind: "done"; url: string; mock: boolean; provider: string; prompt: string };

export interface RenderPaneProps {
  credits: CreditsHandle;
  /** Draw the depth, edge and clay passes from the current view. */
  capture: () => Promise<RenderPasses>;
}

function packLabel(p: Pack): string {
  return p.label ?? `$${(p.amount / 100).toFixed(0)}`;
}

/** A window opened inside the click (so popup blockers allow it), pointed at checkout once the URL is known. */
function preopen(): { go(url: string): boolean; close(): void } {
  const tauri = (globalThis as unknown as { __TAURI_INTERNALS__?: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> } }).__TAURI_INTERNALS__;
  if (tauri) {
    return {
      go: (url) => {
        void tauri.invoke("open_external", { url });
        return true;
      },
      close: () => {},
    };
  }
  let w: Window | null = null;
  try {
    w = window.open("", "_blank");
    if (w) w.opener = null;
  } catch {
    w = null;
  }
  return {
    go: (url) => {
      if (!w || w.closed) return false;
      w.location.href = url;
      return true;
    },
    close: () => w?.close(),
  };
}

export function RenderPane({ credits, capture }: RenderPaneProps) {
  const { state, client } = credits;
  const [preset, setPreset] = useState(0);
  const [prompt, setPrompt] = useState(PRESETS[0].prompt);
  const [result, setResult] = useState<Result>({ kind: "idle" });
  const [busy, setBusy] = useState(false);
  const [buying, setBuying] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryItem[] | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [claim, setClaim] = useState<{ open: boolean; email: string; code: string; sent: boolean; message: string | null; error: boolean }>({ open: false, email: "", code: "", sent: false, message: null, error: false });
  const [, tick] = useState(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (result.kind !== "rendering") return;
    const t = setInterval(() => tick((n) => n + 1), 500);
    return () => clearInterval(t);
  }, [result.kind]);

  useEffect(() => {
    if (state.connection === "unlinked") setClaim((c) => ({ ...c, open: true }));
  }, [state.connection]);

  const cost = state.balance?.render_cost ?? 1;
  const packs = state.balance?.packs?.length ? state.balance.packs : PACKS;

  function notConnected(): string {
    return `Render service not connected (${client.baseUrl}). Renders and credit packs need it; check your connection and try again.`;
  }

  async function loadHistory(): Promise<void> {
    try {
      const h = await client.history(24);
      if (mounted.current) setHistory(h.renders);
    } catch {
      if (mounted.current) setHistory([]);
    }
  }

  async function render(): Promise<void> {
    if (busy) return;
    if (state.connection !== "connected") {
      setResult({ kind: "error", text: state.connection === "unlinked" ? "These credits were moved to another device. Use Move credits below to bring them back here." : state.connection === "unsupported" ? (state.problem ?? "This browser cannot hold a device key.") : notConnected() });
      void credits.refresh();
      return;
    }
    if ((state.credits ?? 0) < cost) {
      setResult({ kind: "info", text: "No credits on this device. Buy a pack below." });
      return;
    }
    setBusy(true);
    const started = Date.now();
    const usedPrompt = prompt.trim();
    try {
      setResult({ kind: "rendering", preview: "", started, stage: "Drawing the depth and edge passes…" });
      const passes = await capture();
      setResult({ kind: "rendering", preview: passes.depthPreview, started, stage: `Rendering ${passes.width} × ${passes.height}…` });
      const r = await client.render({ request_id: newRequestId(), prompt: usedPrompt, preset: PRESETS[preset]?.name ?? null, ...passesToRequest(passes) });
      credits.setCredits(r.balance);
      setResult({ kind: "done", url: r.image_url, mock: r.mock, provider: r.provider, prompt: usedPrompt });
      if (showHistory || history) void loadHistory();
    } catch (err) {
      if (err instanceof NothingToRenderError) setResult({ kind: "info", text: err.message });
      else if (err instanceof ServiceUnreachableError) {
        setResult({ kind: "error", text: notConnected() });
        void credits.refresh();
      } else if (err instanceof ServiceError) {
        const bal = typeof err.body.balance === "number" ? err.body.balance : null;
        if (bal !== null) credits.setCredits(bal);
        if (err.code === "insufficient_credits") setResult({ kind: "info", text: "No credits on this device. Buy a pack below." });
        else if (err.code === "render_failed") setResult({ kind: "error", text: "The render failed and your credit was returned. Try again in a minute." });
        else if (err.code === "device_unlinked") {
          setResult({ kind: "error", text: err.message });
          void credits.refresh();
        } else setResult({ kind: "error", text: err.message });
      } else setResult({ kind: "error", text: err instanceof Error ? err.message : String(err) });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function buy(p: Pack): Promise<void> {
    if (buying) return;
    if (state.connection !== "connected") {
      setResult({ kind: "error", text: notConnected() });
      void credits.refresh();
      return;
    }
    const win = preopen();
    setBuying(p.id);
    try {
      const s = await client.checkout(p.id);
      const opened = win.go(s.url);
      credits.watchCheckout();
      const where = s.mode === "simulated" ? "A simulated checkout (this service has no Stripe keys; nothing is charged)" : "Stripe Checkout";
      setResult({
        kind: "info",
        text: `${where} opened in your browser for ${p.credits} credits (${packLabel(p)}). They appear here as soon as the payment completes.${opened ? "" : " If no tab opened:"}`,
        link: opened ? undefined : { href: s.url, label: "open checkout" },
      });
    } catch (err) {
      win.close();
      setResult({ kind: "error", text: err instanceof ServiceUnreachableError ? notConnected() : err instanceof Error ? err.message : String(err) });
    } finally {
      if (mounted.current) setBuying(null);
    }
  }

  async function claimStep(): Promise<void> {
    const email = claim.email.trim();
    if (!email) return;
    try {
      if (!claim.sent) {
        const r = await client.claimRequest(email);
        setClaim((c) => ({ ...c, sent: true, message: r.message, error: false }));
      } else {
        const r = await client.claimRedeem(email, claim.code);
        credits.setCredits(r.credits);
        void credits.refresh();
        setClaim({ open: false, email: "", code: "", sent: false, message: null, error: false });
        setResult({ kind: "info", text: `Credits moved to this device: ${r.credits} credits.${r.unlinked_devices ? " The least recently used other device was unlinked (two devices per balance)." : ""}` });
      }
    } catch (err) {
      setClaim((c) => ({ ...c, message: err instanceof ServiceUnreachableError ? notConnected() : err instanceof Error ? err.message : String(err), error: true }));
    }
  }

  const elapsed = result.kind === "rendering" ? Math.floor((Date.now() - result.started) / 1000) : 0;
  const svc =
    state.connection === "connected"
      ? `Render service connected · ${state.credits ?? 0} credit${state.credits === 1 ? "" : "s"} on this device${state.awaitingPayment ? " · waiting for checkout" : ""}`
      : state.connection === "connecting"
        ? "Connecting to the render service…"
        : state.connection === "unlinked"
          ? "This device's credits were moved to another device."
          : state.connection === "unsupported"
            ? (state.problem ?? "This browser cannot hold a device key.")
            : "Render service not connected.";

  return (
    <>
      <h3>
        Render <small>current view</small>
      </h3>
      <div className="presets" role="group" aria-label="Presets">
        {PRESETS.map((p, i) => (
          <button
            key={p.name}
            type="button"
            aria-pressed={preset === i}
            onClick={() => {
              setPreset(i);
              setPrompt(p.prompt);
            }}
          >
            {p.name}
            <small>{p.sub}</small>
          </button>
        ))}
      </div>
      <textarea id="prompt" aria-label="Prompt" value={prompt} maxLength={1000} onChange={(e) => setPrompt(e.target.value)} />
      <div className="btnrow">
        <button type="button" className="btn primary" id="renderBtn" disabled={busy} aria-busy={busy} onClick={() => void render()}>
          {busy ? "Rendering…" : `Render · ${cost} credit${cost === 1 ? "" : "s"}`}
        </button>
        <button
          type="button"
          className="btn"
          id="historyBtn"
          aria-pressed={showHistory}
          onClick={() => {
            const next = !showHistory;
            setShowHistory(next);
            if (next) void loadHistory();
          }}
        >
          History
        </button>
      </div>
      <div className={`result${result.kind === "done" ? " done img" : ""}${result.kind === "rendering" ? " busy" : ""}${result.kind === "error" ? " err" : ""}`} id="result" data-state={result.kind} aria-live="polite">
        {result.kind === "idle" ? IDLE_TEXT : null}
        {result.kind === "info" || result.kind === "error" ? (
          <span>
            {result.text}
            {result.link ? (
              <>
                {" "}
                <a href={result.link.href} target="_blank" rel="noopener noreferrer">
                  {result.link.label}
                </a>
              </>
            ) : null}
          </span>
        ) : null}
        {result.kind === "rendering" ? (
          <>
            {result.preview ? <img className="preview" src={result.preview} alt="Depth pass of the current view" /> : null}
            <span className="stage">
              {result.stage} {elapsed > 0 ? `${elapsed} s` : ""}
            </span>
          </>
        ) : null}
        {result.kind === "done" ? (
          <>
            <img src={result.url} alt={`Render: ${result.prompt}`} id="renderImg" />
            <span className="cap">{result.mock ? "Mock render · no provider keys" : `Rendered by ${result.provider}`}</span>
          </>
        ) : null}
      </div>
      {showHistory ? (
        <div className="history" aria-label="Render history">
          {history === null ? <p className="note">Loading…</p> : null}
          {history && history.length === 0 ? <p className="note">No renders on this device yet.</p> : null}
          {history?.map((h) =>
            h.image_url ? (
              <button key={h.request_id} type="button" title={h.prompt} onClick={() => setResult({ kind: "done", url: h.image_url!, mock: (h.provider ?? "").startsWith("mock"), provider: h.provider ?? "", prompt: h.prompt })}>
                <img src={h.image_url} alt={h.prompt} loading="lazy" />
              </button>
            ) : (
              <span key={h.request_id} className="gone" title={h.prompt}>
                {h.status === "refunded" ? "failed · refunded" : h.status === "pending" ? "running" : "expired"}
              </span>
            ),
          )}
        </div>
      ) : null}
      <div className="packs" role="group" aria-label="Credit packs">
        {packs.map((p) => (
          <button key={p.id} type="button" data-pack={p.id} disabled={buying !== null} onClick={() => void buy(p)}>
            <b>{packLabel(p)}</b>
            {p.credits} credits
          </button>
        ))}
      </div>
      <p className="note">
        No account. Packs are tied to this device; a claim code moves them.{" "}
        <button type="button" className="linkish" aria-expanded={claim.open} onClick={() => setClaim((c) => ({ ...c, open: !c.open, message: null }))}>
          Move credits
        </button>
      </p>
      {claim.open ? (
        <form
          className="claim"
          onSubmit={(e) => {
            e.preventDefault();
            void claimStep();
          }}
        >
          <label>
            Email used at checkout
            <input type="email" value={claim.email} autoComplete="email" onChange={(e) => setClaim((c) => ({ ...c, email: e.target.value }))} disabled={claim.sent} />
          </label>
          {claim.sent ? (
            <label>
              Claim code
              <input value={claim.code} placeholder="ABCD-2345" autoComplete="one-time-code" onChange={(e) => setClaim((c) => ({ ...c, code: e.target.value }))} />
            </label>
          ) : null}
          <div className="btnrow">
            <button type="submit" className="btn">
              {claim.sent ? "Move credits here" : "Email me a code"}
            </button>
          </div>
          {claim.message ? <p className={`note${claim.error ? " err" : ""}`}>{claim.message}</p> : null}
        </form>
      ) : null}
      <p className="note svc" data-connection={state.connection} title={state.storage ? storageNote(state.storage) : undefined}>
        {svc}
      </p>
    </>
  );
}
