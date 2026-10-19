#!/usr/bin/env bash
# Write SHA256SUMS.txt for a release's files, and check it.
#
#   scripts/release-checksums.sh <dir>
#
# <dir> holds every file of the release as uploaded (the release workflow downloads the
# draft release's assets into it). Writes <dir>/SHA256SUMS.txt, one "<sha256>  <name>"
# line per file (two spaces, sorted by name, the format `sha256sum -c` reads), then checks
# it. Fails (exit 1) when:
#   - the directory holds no files, a subdirectory, or a name with a space or newline in it;
#   - an installer kind is missing (the Windows .exe and .msi, the macOS .dmg, the Linux
#     .AppImage, .deb and .rpm): a platform's build uploaded nothing;
#   - the list does not name every file exactly once, or `sha256sum -c` disagrees with it.
# An SHA256SUMS.txt already in <dir> (a re-run) is replaced, never listed in itself.
set -euo pipefail

dir="${1:?usage: scripts/release-checksums.sh <dir>}"
[ -d "$dir" ] || { echo "release-checksums: $dir is not a directory" >&2; exit 1; }
cd "$dir"
rm -f SHA256SUMS.txt

files=()
for f in * .[!.]*; do
  [ -e "$f" ] || continue
  if [ -d "$f" ]; then echo "release-checksums: $dir/$f is a directory (expected only release files)" >&2; exit 1; fi
  case "$f" in
    *[[:space:]]*) echo "release-checksums: \"$f\" has whitespace in its name" >&2; exit 1 ;;
  esac
  files+=("$f")
done
[ "${#files[@]}" -gt 0 ] || { echo "release-checksums: no files in $dir" >&2; exit 1; }

missing=0
for kind in '*_x64-setup.exe' '*_x64_en-US.msi' '*_aarch64.dmg' '*_amd64.AppImage' '*_amd64.deb' '*.x86_64.rpm'; do
  found=0
  for f in "${files[@]}"; do
    # shellcheck disable=SC2254
    case "$f" in $kind) found=1 ;; esac
  done
  if [ "$found" = 0 ]; then echo "release-checksums: no $kind among the release files" >&2; missing=1; fi
done
[ "$missing" = 0 ] || exit 1

# One hash per file, read from stdin, so the line format is ours on every platform
# (sha256sum on Windows marks binary mode with "*name").
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
for f in "${files[@]}"; do
  h="$(sha256sum < "$f" | cut -d' ' -f1)"
  [[ "$h" =~ ^[0-9a-f]{64}$ ]] || { echo "release-checksums: no SHA-256 for $f" >&2; exit 1; }
  printf '%s  %s\n' "$h" "$f" >> "$tmp"
done
LC_ALL=C sort -k2 "$tmp" > SHA256SUMS.txt

lines="$(wc -l < SHA256SUMS.txt | tr -d ' ')"
names="$(cut -d' ' -f3- SHA256SUMS.txt | LC_ALL=C sort -u | wc -l | tr -d ' ')"
if [ "$lines" != "${#files[@]}" ] || [ "$names" != "${#files[@]}" ]; then
  echo "release-checksums: SHA256SUMS.txt has $lines lines naming $names files, want ${#files[@]}" >&2
  exit 1
fi
sha256sum --strict -c SHA256SUMS.txt
echo "release-checksums: SHA256SUMS.txt lists ${#files[@]} files"
