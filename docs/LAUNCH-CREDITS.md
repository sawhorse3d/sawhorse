# Before credits go live: what the maintainer provides

The render service and the credit store are built and tested with the mock provider, a simulated checkout, an embedded Postgres and printed claim codes. Nothing below exists yet. Each item names where it goes. Secrets go only into `~/.sawhorse/.env` (local, never committed) and `/etc/sawhorse/render.env` on the production host. Never put them in either repo.

## Accounts and keys

- [ ] **The LLC.** Form the single-member LLC before the first paid pack. Stripe, Resend and the domain should be opened under it.
- [ ] **Stripe account under the LLC.** Business details, bank account, statement descriptor (e.g. `SAWHORSE CREDITS`). Turn on the payment methods you want in Checkout. Card alone is fine to start; the delayed methods are already handled through `async_payment_succeeded`.
- [ ] **Stripe test keys** to `~/.sawhorse/.env`:
  - `STRIPE_SECRET_KEY=sk_test_...`
  - `STRIPE_WEBHOOK_SECRET=whsec_...`. This comes from `stripe listen --forward-to 127.0.0.1:8787/v1/stripe/webhook` locally (install the Stripe CLI), or from the test-mode endpoint on the production host.
  - Then buy each pack once with card `4242 4242 4242 4242` and check that the chip moves by 100, 250 and 500.
- [ ] **Stripe live keys** and the live webhook endpoint (`https://<render host>/v1/stripe/webhook`). Subscribe it to `checkout.session.completed`, `checkout.session.async_payment_succeeded` and `checkout.session.async_payment_failed`. These go only to the production host, after the test-mode run passes.
- [ ] **fal key** (`FAL_KEY`) with billing on. Before go-live, run one real render with a test key and check `src/providers/fal.ts` against fal's current schema for `fal-ai/flux-control-lora-depth`: endpoint path, `control_lora_image_url`, `preprocess_depth`, `image_size`, `output_format`, and the response's `images[0].url`. The field names were written from memory; nothing external was read while building.
- [ ] **Replicate token** (`REPLICATE_API_TOKEN`) with billing on. Check `src/providers/replicate.ts` against `black-forest-labs/flux-depth-dev` in the same way: `control_image`, `aspect_ratio`, `megapixels`, `output_format`, `Prefer: wait` and the `output` shape.
- [ ] **Provider cost check.** Packs are 10 cents a credit. Confirm one render's provider cost, retries included, leaves margin. If it does not, raise the cost per render (`RENDER_COST` in `src/packs.ts`), not the pack prices. ADR-0004 says never sell a pack under $10.
- [ ] **Resend key** (`RESEND_API_KEY`) and a verified sending domain, then set `EMAIL_FROM=Sawhorse <credits@<domain>>`. Without them, production turns claim codes off (503) and says so.
- [ ] **Postgres.** Either a managed Postgres (ADR-0004: a dedicated role and database or schema, session-pooler connection string with `sslmode=require`) or a Postgres on the production host. Put the string in `DATABASE_URL`. Migrations run on start.
- [ ] **`RENDER_URL_SECRET`**: `openssl rand -base64 48`, production host only.

## DNS and hosting

- [ ] **A host name for the service**, e.g. `render.<sawhorse domain>`, with an `A` record to the production host's public IP. Stripe and the providers must reach it from the internet, so it cannot be 
- [ ] **TLS reverse proxy** on the production host (Caddy or the existing one) to `127.0.0.1:8787`, with a 25 MB body limit and no path rewriting. See `render-service/docs/DEPLOY.md`.
- [ ] **App configuration for production:**
  - Build with `VITE_RENDER_SERVICE_URL=https://<render host>` (CI release workflow env).
  - Add `https://<render host>` to `connect-src` and `img-src` in `src-tauri/tauri.conf.json`.
  - Add the hosted web demo's origin, if any, to `ALLOWED_ORIGINS` on the service.
- [ ] **Deploy** per `render-service/docs/DEPLOY.md`, the maintainer's go only. Then run the smoke test there: healthz, a test-mode purchase, one real render, and a webhook redelivery that must not change the balance.

## Policy and copy

- [ ] **Terms of sale and a refund policy** linked from Checkout (Stripe Dashboard > Settings > Public details): credits do not expire, failed renders are re-credited automatically, and say how refunds of unused packs work.
- [ ] **Privacy note**: what leaves the machine (prompt plus depth, edge and clay images of the current view), how long images are kept (7 days, `IMAGE_RETENTION_HOURS`), and that the checkout email is used only for receipts and claim codes.
- [ ] **Alerting**: route the service's `"alert":true` log lines (a paid session that matched no pack, a failed claim email) to email. The existing alerting on that server does not work yet, so this needs a working path first.

## Already done (no action)

- Signed requests with a 5-minute skew window and a nonce cache.
- Idempotent fulfilment keyed by Checkout Session id, with guards against pack/amount mismatch and live/test mode mismatch.
- Atomic decrement that never goes negative, tested on real Postgres.
- Refund when every provider fails, and a sweeper that refunds stuck renders.
- Limits: 2 concurrent, 6 a minute, 200 a day.
- Claim codes: single use, bound to the asking device, five tries, 15 minutes, three emails an hour, two devices per balance.
- Production refuses to start on placeholder secrets.
- Dockerfile, systemd unit and deploy guide.
