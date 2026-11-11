# Contributing to Sawhorse

Help is welcome: a report, a small fix, a larger piece of work, or a morning spent testing a build on hardware the maintainer does not have. This page says how each kind of help fits, so that what you send can go in without a second round.

Sawhorse is one maintainer's project, published under a pseudonym. The maintainer uses AI agents heavily, under rules you can read in the history: one piece of work (a "lane") at a time, an independent verifier that runs its own fuzzing and tries to break each change by hand, and the docs and tests in the same commit as the code.

## Reports

Open an issue whenever something is wrong, odd or missing. Say which version you ran (the title bar, or the release you downloaded), on which system (Windows, macOS or Linux, and on Linux the desktop and whether it is Wayland or X11), and the steps that show it. A report like that is worth as much as the fix: #31 and #32, two desktop bugs only Linux showed, came from someone scripting the Linux build and were easy to see because the steps were there.

## Pull requests

**Small fixes** (a bug, a wrong sentence in the guide, a test that is missing): open a pull request straight away. There is no need to ask first.

**Larger items and your own ideas** (anything from the README's "Not yet" list, a new tool, a new pane, a change to how something works): open an issue first. Say what you would build and how it would look to the person using it: what they click or type, and what they see. The maintainer answers quickly, either with "go" or with the one or two things that would make it fit (usually a question about the file format or the kernel). That saves you building something you would then be asked to reshape. Ideas from points of view the maintainer lacks, a Linux desktop for one, are wanted.

**AI-assisted contributions are welcome.** Code written with Claude or another model is not a problem here. Say so in the pull request, keep the model's trailer in the commits (`Co-Authored-By: ...`), and say what was run and what was not ("the web tests and Playwright, headless; the desktop shell not built"). A plain account of what you could not check is more useful than a claim that everything passed.

## What makes a change fit

- **One change per pull request**, with a test that fails without it and passes with it. If you are not sure the test fails without the change, run it once against `main` and say so.
- **A changelog entry** under `## [Unreleased]` in `CHANGELOG.md`, in the house form: a dated heading (`### Fixed (what it is, YYYY-MM-DD)`), then a bold line saying what changed for the person using Sawhorse, with the issue linked after it; what they would have lost before; how it works now; and which tests cover it. The entries already there show the shape.
- **The docs with the behaviour.** When what a person sees or does changes, `docs/USER-GUIDE.md` changes in the same pull request. When a field in a saved file changes, `docs/format.md` does. When a decision recorded in `docs/adr/` changes, its ADR gets a dated note.
- **The full gate green** where you can run it, and a line saying which parts you could not. The Rust commands run in `crates/` (on Windows without a C++ linker, `bash scripts/cargo.sh <the same arguments>` runs them inside WSL; see `docs/DEVELOPMENT.md`):

  ```bash
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --check
  cargo check --workspace --target wasm32-unknown-unknown
  ```

  and from the repository root:

  ```bash
  npm ci
  npm run wasm
  npx vitest run
  npm run build
  npm run golden:wasm
  npm run boolean:diff
  npm run corpus:skp
  npm run site && npm run site:check
  npx playwright test
  ```

  A change under `crates/kernel` also runs the long fuzz CI runs: `FUZZ_CASES=10000 FUZZ_SEED=20261011 cargo test -p sawhorse-kernel --test sequences -- --ignored long_fuzz`. The maintainer reruns all of it before anything is merged, so a part you could not run is not a reason to hold the pull request back; just say which.
- **Never loosen a tolerance or a timeout, and never delete an assertion** to make a test pass. If a test is wrong, say why in the pull request and change what it checks.
- **No change to a golden hash or to the `.skp` corpus bytes** (`crates/kernel/tests/golden/`, `tests/corpus/`) without saying why in the pull request. A golden that moves is a model that saves or draws differently, and every saved file depends on that.
- **No new dependency without an issue first.** Each one is code the app ships and someone has to keep up to date.
- **The clean-room rule** in [`docs/CLEANROOM.md`](docs/CLEANROOM.md): nobody who writes code here reads SketchUp's SDK or another program's `.skp` reader, so that Sawhorse's own stays its own. If you have read one, say so in the issue before writing the code, and someone else will write that part.

## How a pull request lands

Pull requests land squashed: one commit on `main` in the house form (a subject like `fix(files): ...`, a body saying what was wrong and how it works now, the tests, `Fixes #<issue>`), with you as co-author through your GitHub no-reply address. That is why the merged commit's hash differs from yours. If the maintainer adds a test or a line of docs while squashing, the commit says so.

## What the maintainer keeps

- **The `.shz` file format and the kernel's model hash.** Every saved file depends on both, so a change to either goes through an issue first and usually an ADR.
- **Releases, signing and the site** (sawhorse3d.com).

Everything else is open.

## Testing on real hardware

Most of the testing so far has been headless. Reports from real machines are wanted, especially a Linux desktop with a GPU and Wayland, macOS, and Windows:

- the `.deb` and the AppImage under Wayland: the window opens, resizes and goes fullscreen; the native GTK dialogs (Open, Save, Save As, Import) come up and return the right paths;
- HiDPI with fractional scaling: the cursor and the point it picks line up, and the panel text is sharp;
- orbit, pan and zoom with a mouse and with a touchpad (the guide's Navigating section says what each gesture should do);
- a large model: import the biggest `.skp` you have, orbit it, run the cut list and the nesting, and say where it gets slow;
- Save, close and reopen: the same model, with the same hash in the title;
- the question before unsaved changes are thrown away, on a real display.

One line per item, pass or fail, with the version and the compositor (or the system), is the most useful report this project can get.

## Licence

Sawhorse is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option, as the README says. Contributions are accepted under the same terms: by opening a pull request you agree that your contribution may be used under either licence.
