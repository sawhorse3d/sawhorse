# Code signing policy

Sawhorse's installers are not code-signed today: Windows and macOS warn before the first launch, and the release notes walk through it ([alpha.6](releases/v0.1.0-alpha.6.md#unsigned-installers)). This page says how signing will work and who decides what gets signed.

## Who signs, and where

Sawhorse's Windows installers are to be signed by [SignPath Foundation](https://signpath.org/) once the application is approved. Signing is performed by SignPath's service on artifacts built by GitHub Actions (`.github/workflows/release.yml`) from a tagged commit of the public repository, [github.com/sawhorse3d/sawhorse](https://github.com/sawhorse3d/sawhorse). No maintainer machine is involved in building or signing: what is signed is exactly what the workflow built from that commit, and the build log is public. The macOS build carries only an ad-hoc signature (below) and the Linux builds none for now; their checksums are in each release's `SHA256SUMS.txt`.

**Until the certificate (macOS).** On Apple Silicon an app with no valid signature at all is reported as "damaged and can't be opened", and Gatekeeper never offers Open Anyway ([#27](https://github.com/sawhorse3d/sawhorse/issues/27); alpha.3 and alpha.4). Since lane 28 (0.1.0-alpha.5 is the first release built this way, and it is not yet confirmed on a real Mac) the release workflow signs the macOS app ad hoc while no Apple identity is set (`APPLE_SIGNING_IDENTITY=-`, so Tauri signs the `.app` before it makes the `.dmg`), then checks the app and the app inside the `.dmg` with `codesign --verify --deep --strict`, signing, rebuilding and re-uploading them if they do not verify (`scripts/macos-adhoc-check.sh`). An ad-hoc signature says nothing about who built the app; it only makes it openable, through the usual Open Anyway steps. For a build that says damaged, the release notes give the two commands: `xattr -cr /Applications/Sawhorse.app`, and if that is not enough, `codesign --force --deep --sign - /Applications/Sawhorse.app`. A Developer ID certificate and notarization replace all of this.

Every release also carries `SHA256SUMS.txt`, the SHA-256 of every file the workflow uploaded, so a download can be checked against the release whether it is signed or not.

## Roles

- **Author, Reviewer and Approver**: the maintainer, [@boardfoot](https://github.com/boardfoot).
- **Changes by anyone else** arrive as pull requests and are reviewed before they are merged. Nothing reaches `main` from outside without that review.
- **A release is signed only** after the full CI gate (`.github/workflows/ci.yml`: the Rust tests, clippy, the fuzzer, the web build, the unit and end-to-end tests) and the release verification pass are green on the tagged commit.

## Privacy

Sawhorse sends nothing off the machine. There is no telemetry and no account. Your models stay in the files you save and in the app's own autosave on your machine.

- The optional render service is contacted only if you enter its address in **Edit > Settings**; with no address set, the app shows nothing about it and never calls it.
- A plugin can reach the network only if it declares the `net` permission and you grant it when you enable the plugin, and then only the origins its manifest lists ([PLUGINS.md](PLUGINS.md)).
- On Windows, the installer downloads Microsoft's WebView2 runtime if the system does not have it yet; Windows 10 and 11 usually do.

## Uninstalling

- **Windows**: the `.exe` and `.msi` installers register with **Settings > Apps > Installed apps** (Apps & features); uninstall Sawhorse there.
- **macOS**: drag Sawhorse from Applications to the Trash.
- **Linux**: remove the Sawhorse package installed from the `.deb` or `.rpm` with your package manager; the AppImage is a single file, so deleting it removes the app.

Your saved `.shz` files are yours and are never removed by uninstalling.
