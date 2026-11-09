#!/usr/bin/env bash
# Lane 28 (#27): until the app has an Apple Developer ID, every macOS build must at least
# carry a valid ad-hoc signature. On Apple Silicon an app whose signature is missing or
# broken is "damaged and can't be opened", and Gatekeeper never offers Open Anyway, so the
# steps in the release notes never appear.
#
#   bash scripts/macos-adhoc-check.sh <tauri bundle dir> [<release tag>]
#
# The release workflow sets APPLE_SIGNING_IDENTITY to "-" when no identity secret is set, so
# tauri-bundler signs the .app ad hoc itself, before it makes the .dmg from it (the only
# point between the two). This script runs after tauri-action has built and uploaded:
#
#   1. it verifies Sawhorse.app and the Sawhorse.app inside the .dmg (codesign --verify
#      --deep --strict) and prints each signature;
#   2. if either is unsigned or invalid and the build had no real identity, it signs the
#      .app ad hoc (codesign --force --deep --sign -), makes the .dmg and the app archive
#      again from it, uploads both over the draft release's copies (with a tag), and
#      verifies again. With a real identity, or with an updater signature (.sig) that a new
#      archive would no longer match, it fails instead of papering over the build.
#
# Exits non-zero when what ships would not carry a valid signature. Untested on a runner
# until the next release build (it needs macOS: codesign, hdiutil).
set -euo pipefail

bundle="${1:?usage: macos-adhoc-check.sh <tauri bundle dir> [<release tag>]}"
tag="${2:-}"
app="$bundle/macos/Sawhorse.app"
dmg="$(ls "$bundle"/dmg/*.dmg 2>/dev/null | head -n 1 || true)"
[ -d "$app" ] || { echo "macos-adhoc-check: no $app" >&2; exit 1; }
[ -n "$dmg" ] || { echo "macos-adhoc-check: no .dmg in $bundle/dmg" >&2; exit 1; }

verify() {
  # A valid signature on the bundle and on everything inside it, then who signed it.
  if codesign --verify --deep --strict --verbose=2 "$1"; then
    codesign --display --verbose=2 "$1" 2>&1 | grep -E '^(Identifier|Format|Signature|Authority|TeamIdentifier)=' || true
    return 0
  fi
  return 1
}

verify_dmg() {
  local mnt
  mnt="$(mktemp -d)"
  hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$dmg" >/dev/null
  local ok=0
  echo "== $(basename "$dmg"): Sawhorse.app"
  verify "$mnt/Sawhorse.app" || ok=1
  hdiutil detach "$mnt" >/dev/null || hdiutil detach -force "$mnt" >/dev/null
  return "$ok"
}

echo "== $app"
if verify "$app" && verify_dmg; then
  echo "macos-adhoc-check: the app and the disk image's app are signed"
  exit 0
fi

identity="${APPLE_SIGNING_IDENTITY:-}"
if [ -n "$identity" ] && [ "$identity" != "-" ]; then
  echo "macos-adhoc-check: signed with an Apple identity, and the signature does not verify; not re-signing" >&2
  exit 1
fi

echo "macos-adhoc-check: no valid signature; signing ad hoc"
codesign --force --deep --sign - "$app"
echo "== $app (re-signed)"
verify "$app"

# The disk image again, laid out as tauri's: the app and a link to /Applications.
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname Sawhorse -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"
upload=("$dmg")

# The app archive tauri uploads beside the disk image, when there is one.
archive="$bundle/macos/Sawhorse.app.tar.gz"
if [ -f "$archive" ]; then
  if [ -f "$archive.sig" ]; then
    echo "macos-adhoc-check: $archive has an updater signature a new archive would not match; not rebuilding" >&2
    exit 1
  fi
  tar -czf "$archive" -C "$bundle/macos" Sawhorse.app
  # tauri-action uploads it as <name>_<arch>.app.tar.gz.
  named="$bundle/macos/Sawhorse_aarch64.app.tar.gz"
  cp "$archive" "$named"
  upload+=("$named")
fi

verify_dmg

if [ -n "$tag" ]; then
  gh release upload "$tag" "${upload[@]}" --clobber --repo "${GITHUB_REPOSITORY:?}"
  echo "macos-adhoc-check: uploaded ${upload[*]} over the draft release's copies"
else
  echo "macos-adhoc-check: no release tag given; nothing uploaded"
fi
