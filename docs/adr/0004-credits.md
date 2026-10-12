# ADR-0004: Credits and render service

- Status: Accepted, 2026-10-11 (seed decisions). Implemented in lane 8, 2026-10-11 (app side here; service in `../render-service`, built and tested, not deployed). See `docs/RENDER.md` and `docs/LAUNCH-CREDITS.md`.
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

## Implementation status (lane 8, 2026-10-11)

- **Device key:**
  - Ed25519. Desktop: the seed is in the OS secure store via `src-tauri/device-key` (`keyring` 3.6; Windows Credential Manager, macOS Keychain, Secret Service), behind the `device_public_key` and `device_sign` commands.
  - Browser demo: a non-extractable WebCrypto key in IndexedDB, or a key for this tab only when storage is blocked. The chip's tooltip says which.
- **Signed requests:**
  - Headers `X-Install-Id`, `X-Public-Key`, `X-Timestamp`, `X-Nonce`, `X-Signature`.
  - The canonical string is `sawhorse-v1`, method, path with query string, SHA-256 of the body, timestamp and nonce.
  - 5-minute skew, and a nonce table pruned after 11 minutes. One vector is tested in TypeScript, Rust and the service.
- **Service** (`../render-service`):
  - Hono on Node 24; PGlite locally and Postgres in production, with the same SQL.
  - Tables: `balances`, `device_keys`, `fulfillments` (primary key = Checkout Session id), an append-only `ledger`, `renders`, `nonces`, `claim_codes`.
  - Webhook: `checkout.session.completed` (only when `payment_status=paid`) and `async_payment_succeeded`; pack, amount and livemode guards.
  - `/v1/render`: limits and decrement in one transaction under a row lock on the balance; fal, then Replicate, each tried twice; refund on failure, plus a sweeper for stuck renders.
  - Claim codes: single use, bound to the asking device, 15 minutes, five tries, three emails an hour, at most two devices per balance (the least recently seen is unlinked).
- **Device binding:** unlinked devices get `403 device_unlinked` and can claim back.
- **Not yet:** real keys, DNS, deployment and the production CSP origin (`docs/LAUNCH-CREDITS.md`).
