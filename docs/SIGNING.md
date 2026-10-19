# Code signing policy

Sawhorse's installers are not code-signed today: Windows and macOS warn before the first launch, and the release notes walk through it ([alpha.3](releases/v0.1.0-alpha.3.md#unsigned-installers)). This page says how signing will work and who decides what gets signed.

## Who signs, and where

Sawhorse's Windows installers are to be signed by [SignPath Foundation](https://signpath.org/) once the application is approved. Signing is performed by SignPath's service on artifacts built by GitHub Actions (`.github/workflows/release.yml`) from a tagged commit of the public repository, [github.com/sawhorse3d/sawhorse](https://github.com/sawhorse3d/sawhorse). No maintainer machine is involved in building or signing: what is signed is exactly what the workflow built from that commit, and the build log is public.

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
