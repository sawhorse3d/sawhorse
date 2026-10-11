# ADR-0004: Credits and render service

- Status: Accepted, 2026-10-11 (seed decisions). Not implemented yet (credit-store lane).
- Deciders: the maintainer
- Source: `docs/DECISIONS.md` seed

## Context

The app is free and open source. AI renders cost real money per call, so they are sold as prepaid credit packs. There are no user accounts, and provider keys must never ship inside the app.

## Decision

- No user accounts. The app generates an Ed25519 device keypair stored in the OS secure store; `install_id` = base32(sha256(pubkey))[:20].
- Stripe Checkout (mode=payment, `client_reference_id` = install_id, guest customer) for packs $10/100, $25/250, $50/500. Never a pack under $10.
- Webhook (`checkout.session.completed`, `checkout.session.async_payment_succeeded`) is the source of truth; idempotent fulfilment table keyed by session id; server-side ledger in Postgres (personal managed Postgres). Signed requests (install id, timestamp, nonce, signature); nonce cache; 5-minute skew.
- `/v1/render`: decrement atomically, call fal `flux-control-lora-depth` (depth map from the engine, preprocess off), fall back to Replicate `flux-depth-dev` (clay render), re-credit on failure. Device binding max 2 devices; rebind via emailed claim code; limits 2 concurrent / 6 per minute / ~200 per day.
- Provider keys live only on the server (`~/.sawhorse/.env` locally, the production host in production). The desktop app never holds them.

## Consequences and implementation status

- Lane 1 shows the Render pane and Credits chip from the mock, inert: the chip reads 0, Render and the pack buttons explain that the service and store are not connected, and nothing is charged or simulated.
- The Tauri capability file grants `core:default` only; the secure-store and HTTP permissions this ADR needs are added by the credit-store lane, which also owns `render-service/` (outside this repo folder).
- Release signing secrets in `.github/workflows/release.yml` are unrelated to this ADR's keys and are optional.
