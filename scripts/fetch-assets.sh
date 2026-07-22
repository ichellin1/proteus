#!/usr/bin/env bash
# Downloads the reference demo's box-cover images and video clips from a
# GitHub Release and places them where both shells expect to find them.
#
# These assets aren't committed to the repo (GitHub has file-size limits and
# they're not source anyway) — see the "Build & Run" section of README.md.
#
# Usage:
#   scripts/fetch-assets.sh [release-tag]
#
# release-tag defaults to $DEMO_ASSETS_TAG or "demo-assets-v1" if unset.

set -euo pipefail

REPO="ichellin1/proteus"
TAG="${1:-${DEMO_ASSETS_TAG:-demo-assets-v1}}"
ASSET_NAME="demo-assets.zip"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NATIVE_IMAGES="$ROOT/crates/proteus-shell-native/images"
NATIVE_VIDEOS="$ROOT/crates/proteus-shell-native/assets/videos"
WEB_IMAGES="$ROOT/crates/proteus-shell-web/www/images"
WEB_VIDEOS="$ROOT/crates/proteus-shell-web/www/videos"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

echo "Fetching demo assets from $URL"
if ! curl -fL --progress-bar -o "$WORKDIR/$ASSET_NAME" "$URL"; then
  echo "error: download failed. Check that release '$TAG' exists at" >&2
  echo "  https://github.com/${REPO}/releases" >&2
  exit 1
fi

unzip -q "$WORKDIR/$ASSET_NAME" -d "$WORKDIR/extracted"

mkdir -p "$NATIVE_IMAGES" "$NATIVE_VIDEOS" "$WEB_IMAGES" "$WEB_VIDEOS"

# Images: same filenames on both shells.
cp "$WORKDIR"/extracted/images/*.jpg "$NATIVE_IMAGES/"
cp "$WORKDIR"/extracted/images/*.jpg "$WEB_IMAGES/"

# Videos: web uses the plain filename; native still expects the "_fixed"
# suffix left over from the ffmpeg re-encode step (see mp4_player.rs comments
# in proteus-shell-native — TILE_VIDEO_PATHS). Same bytes, two names.
for f in "$WORKDIR"/extracted/videos/*.mp4; do
  base="$(basename "$f" .mp4)"
  cp "$f" "$WEB_VIDEOS/${base}.mp4"
  cp "$f" "$NATIVE_VIDEOS/${base}_fixed.mp4"
done

echo "Done. Assets placed under:"
echo "  $NATIVE_IMAGES"
echo "  $NATIVE_VIDEOS"
echo "  $WEB_IMAGES"
echo "  $WEB_VIDEOS"
