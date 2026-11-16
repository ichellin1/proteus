#!/usr/bin/env bash
# Downloads the fonts the harness uses into fonts/, from Google Fonts' GitHub
# repository, and checks them against the checksums they were measured with.
# All are under the SIL Open Font License 1.1. They're kept out of git.

set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p fonts
cd fonts

base=https://github.com/google/fonts/raw/main/ofl
fetch() {
  [ -f "$1" ] || curl -sSLf -o "$1" "$2"
}
fetch 'Inter[opsz,wght].ttf' "$base/inter/Inter%5Bopsz%2Cwght%5D.ttf"
fetch 'Inter-Italic[opsz,wght].ttf' "$base/inter/Inter-Italic%5Bopsz%2Cwght%5D.ttf"
fetch 'NotoSansDevanagari[wdth,wght].ttf' "$base/notosansdevanagari/NotoSansDevanagari%5Bwdth%2Cwght%5D.ttf"

shasum -a 256 -c <<'SUMS'
29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031  Inter[opsz,wght].ttf
acd98e64795781b2058f07b18475e0ecee2a0fe2b42a49e2f9e37d0d6bf66ce6  Inter-Italic[opsz,wght].ttf
14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e  NotoSansDevanagari[wdth,wght].ttf
SUMS
