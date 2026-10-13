# Render panel and credits

How the Render pane, the Credits chip and the render service fit together (ADR-0004). The service lives in its own repository, `../render-service/` (README there: API, tests, deploy).

## What the user sees

- **Nothing, until a render service is set and answers** (lane 15). Render credits are not launched, so a first run shows no Credits chip and no Render pane. They appear when a render service address is saved in **Edit > Settings…** (kept on this device; a development build, `npm run dev`, also takes `VITE_RENDER_SERVICE_URL`; since lane 12c it has no default address, so with nothing set nothing is asked) **and** that service answers `GET /healthz` with a 2xx within 4 s, asked once at startup (only when an address is configured) and whenever the address changes (**Check again** asks once more). Otherwise one line in Settings says which: "Renders are off: no render service is set." or "...the render service at URL did not answer its health check." A release build never falls back to a built-in address. Render images are fetched and shown from memory (`blob:`), so a service set at run time works under the page's content security policy, which lets images come only from the app.
- **Credits chip** (top bar): the balance on this device. It shows a dash while the render service is not connected, and clicking it opens the Render pane. Its tooltip says where the device key is kept.
- **Render pane**: the approved mock's layout.
  - Presets (Workshop, Kitchen, Living room, Exterior) fill the prompt box.
  - **Render · 1 credit** renders the current view.
  - The result box shows the depth pass faintly while the render runs, then the image. A mock render is labelled as one.
  - **History** opens a gallery of this balance's recent renders.
  - **$10 / $25 / $50** open Stripe Checkout in the system browser. The chip follows the payment by polling for up to ten minutes and on window focus.
  - **Move credits** asks for the checkout email, emails a claim code, and moves the balance to this device. A balance can be on two devices at most; a third claim unlinks the device used least recently.
- **When the service cannot be reached**, the pane says "Render service not connected (URL)". Nothing is charged or simulated. The app asks again after 10 s, 30 s and 60 s, then every 5 minutes (and when you come back to the window), so starting the service later connects without a reload. These attempts are logged at debug level only (`[credits] ...`); the browser may still print its own line for each failed request, which the back-off keeps to a handful an hour.

## What leaves the machine

The prompt, the preset name and three images of the current view, each at the view's aspect with the long side 1024 px and both sides multiples of 16:

| Pass | Format | How it is made (`src/render/passes.ts`) |
|---|---|---|
| depth | 16-bit greyscale PNG | Linear view depth of the model only. The nearest point is 65535 and the farthest model point is 8192 (1/8 grey). The background is 0. A shader packs depth into two bytes in an exact (non-antialiased) render target. fal's depth LoRA takes this with its own estimator switched off. |
| edges | 8-bit greyscale PNG | The model's edges as black lines on white, with faces drawn white to hide what is behind them (4x MSAA). |
| clay | 8-bit greyscale PNG | Flat-shaded light clay under a key light from the view's upper left. Replicate's fallback model estimates its own depth from this. |

Plus **material hints** (lane 10): which materials the visible faces show and how much of the picture each covers, so the render can match the wood.

| Field | What | How it is made |
|---|---|---|
| `material_hints` | `[{ name, species, color, share }]`, most of the picture first | An exact material-id pass over the same camera (each face drawn with its material's index, no anti-aliasing), counted over the model's pixels. `species` is the material's stock link (`White oak`, `Baltic birch ply`) or null; `color` its sRGB colour as `#rrggbb`; `share` the fraction of the model's pixels, to 0.001. |
| `prompt` | The prompt typed, then a sentence naming the materials | `Materials: white oak (most of it), baltic birch ply.`: up to five materials covering at least 3% of the model, "(most of it)" when the first covers half or more. Left off when nothing in view is painted, or when it would take the prompt past the service's 1000 characters. |

The service ignores fields it does not know, so `material_hints` is there for it to use; the sentence works today. No model file, texture image or anything else is sent. Canvas cannot write 16-bit PNGs, so `src/render/png.ts` writes them. It uses the platform's `CompressionStream("deflate")`.

The camera is the view's camera, re-aimed at the output aspect, so the render frames what the viewport frames. Zoom in (or press Shift+Z) before rendering to fill the picture.

## Device key and signing (`src/credits/`)

- `device.ts`: the Ed25519 device key and `install_id = base32(sha256(public key))[:20]` (lowercase RFC 4648, no padding). The canonical request string is `sawhorse-v1`, method, path with query string, hex SHA-256 of the body, unix timestamp and nonce, joined by newlines.
  - **Desktop (Tauri):** the 32-byte seed is kept in the OS secure store by the `src-tauri/device-key` crate through `keyring`: Windows Credential Manager, the macOS Keychain, or Secret Service on Linux. The webview never sees it; it calls `device_public_key` and `device_sign`.
  - **Browser demo:** a non-extractable WebCrypto key is stored in IndexedDB. Scripts on this origin can use it to sign, but cannot read it. Clearing site data loses the key and the credits bound to it, unless they were moved with a claim code first. The chip's tooltip says so.
  - **No storage** (private windows, blocked site data): the key lasts for this tab only, and the tooltip warns before anyone buys.
- `client.ts`: `RenderServiceClient` signs every call. Errors are typed: `ServiceUnreachableError`, or `ServiceError` with the service's stable `error` code. If the service refuses a request for clock skew, the client corrects its clock from the response's `Date` header and retries once. In Tauri, `openExternal` sends Checkout to the system browser through the `open_external` command, which only opens `https://checkout.stripe.com/...` and the service's loopback dev checkout.
- `useCredits.ts`: one connection and balance, shared by the chip and the pane.

The same vector (seed `00..1f`, a `POST /v1/checkout` request) is tested in three places, and all three must produce identical bytes:

- `src/credits/vector.test.ts` (WebCrypto)
- `src-tauri/device-key/src/lib.rs` (ed25519-dalek)
- `render-service/test/vector.test.ts` (node:crypto)

## Configuration

- `VITE_RENDER_SERVICE_URL` sets the service for a development build (for the local service, `http://127.0.0.1:8787`). Unset, the app asks no service until one is saved in Settings.
- Tauri's CSP allows `http://127.0.0.1:8787` in `connect-src` and `img-src`. The production origin must be added there at launch; see `docs/LAUNCH-CREDITS.md`.

## Tests

| What | Where |
|---|---|
| Shared signing vector, base32, base64url | `src/credits/vector.test.ts` |
| Signed headers verify, Retry-After, skew correction, unreachable | `src/credits/client.test.ts` |
| 16-bit and 8-bit PNG writer, pass sizes, camera re-aim, the materials sentence | `src/render/png.test.ts` |
| The pane: render, refunds, limits, no credits, not connected, checkout window and polling, popup blocked, history, claim | `src/components/RenderPane.test.tsx` |
| The browser build with the service faked in Playwright. Every signature is verified the way the service verifies it, and the real depth, edge and clay passes are decoded and checked. | `e2e/render.spec.ts`, `e2e/fakeRenderService.ts` |
| Device key: shared vector, create-once, corrupt seed kept, a keychain that drops writes, URL allow-list | `cd src-tauri && cargo test -p sawhorse-device-key` (CI `tauri-check`; `-- --ignored` for the real OS keychain) |

## End to end locally (real service)

```bash
# terminal 1: the service (mock provider, embedded Postgres, simulated checkout)
cd ../render-service && npm ci && PORT=8787 RENDER_PROVIDER=mock node src/main.ts | tee service.log
# terminal 2: the app on its own port
npx vite --port 5178 --strictPort
# terminal 3: drive both and take screenshots
node scripts/capture-credits.mjs --url http://localhost:5178 --service-log ../render-service/service.log \
     --out-dir ../_captures --prefix 2026-10-11-lane08
```

The capture script walks one full session. Run on 2026-10-11, it gave:

1. The app connects; the chip shows 0.
2. It buys the $10 pack. The service's simulated checkout sends a signed `checkout.session.completed` webhook through the real verification path, and the chip reaches 100.
3. It models a 36 x 12 x 48 in carcass with a 9 in deep opening and renders it with the Workshop preset. The chip drops to 99 and the mock image (1024 x 768) appears.
4. It opens History.
5. A second browser profile, which is a second device key, claims the balance with the code from the service log (the service has no Resend key locally). It shows 99.

Screenshots: `../_captures/2026-10-11-lane08-01-connected.png` through `-09-claimed-on-second-device.png`.

To simulate a Stripe delivery from a shell instead (it is idempotent: `--repeat 2` credits once):

```bash
cd ../render-service && node scripts/simulate-webhook.ts --install <install id> --pack p250 --repeat 2
```

## What is not exercised without real keys

These need the maintainer's accounts; see `docs/LAUNCH-CREDITS.md`:

- Real Stripe Checkout (test mode, then live)
- fal and Replicate calls. The adapters' request shapes are unit-tested against fakes; the field names were written from memory and must be checked against the live schemas with a test key.
- Resend email delivery
- The OS keychain on Windows and macOS. CI compiles the Linux path; the `--ignored` test is the manual check.
- The Tauri shell itself, which CI builds (no MSVC locally)
