#!/usr/bin/env bash
# Converts the demo's videos, crates/proteus-demo/assets/videos/*.mp4, into
# simple (single-rendition, no adaptive bitrate) fMP4 HLS for the web shell's
# HLS player, in crates/proteus-shell-web/www/videos/hls/. The native shell
# plays the mp4s directly. Run it again after replacing a video, and commit
# the output.
#
# Re-encodes video (can't just -c copy remux): source keyframe spacing is
# wildly uneven across these three files — jellyfish.mp4 in particular has
# only 2 keyframes in its whole 10s (GOP ~8.3s), so a copy-remux can only
# cut segments at existing keyframes, producing one giant first "segment"
# that's most of the clip and defeating the entire point of segmenting.
# Forcing a fixed ~2s GOP (-g/-keyint_min matched to each file's fps,
# -sc_threshold 0 so scene-cut detection can't insert extra keyframes and
# throw the spacing off again) guarantees HLS can actually cut ~2s chunks.
# Audio (tiger only) is a plain copy — AAC frames are tiny and don't need
# forced keyframes to segment cleanly.
#
# -hls_segment_options movflags=+frag_keyframe+empty_moov+default_base_moof:
# ffmpeg's default fmp4 HLS output plays fine in Chrome but is looser than
# what Safari's (WebKit) MSE implementation tolerates — WebKit is much
# stricter about CMAF-style fragment structure than Chromium is. These are
# the standard flags for making fmp4 fragments broadly MSE-compatible:
# frag_keyframe (fragment only at keyframes, matching our forced GOP above),
# empty_moov (init segment carries no sample data, just track setup —
# required for streamed/segmented playback), default_base_moof (each
# fragment's `trun` byte offsets are relative to its own `moof` rather than
# the whole file, which is what a standalone .m4s segment needs since it
# has no "whole file" to be relative to).
#
# Usage: scripts/make-hls.sh, from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."
videos=crates/proteus-demo/assets/videos
hls=crates/proteus-shell-web/www/videos/hls

for name in tiger jellyfish sintel; do
  src="${videos}/${name}.mp4"
  outdir="${hls}/${name}"
  # Approximate source fps per file (from ffprobe), used to pick a GOP size
  # landing close to hls_time's 2s target. Plain case instead of an
  # associative array — macOS ships bash 3.2, which predates them.
  case "${name}" in
    tiger)     g=48 ;;
    jellyfish) g=60 ;;
    sintel)    g=48 ;;
  esac
  echo "== ${name} (g=${g}) =="
  rm -rf "${outdir}"
  mkdir -p "${outdir}"
  ffmpeg -hide_banner -loglevel warning -y \
    -i "${src}" \
    -map 0:v:0 -map 0:a:0? \
    -c:v libx264 -preset veryfast -crf 20 -g "${g}" -keyint_min "${g}" -sc_threshold 0 \
    -c:a copy \
    -f hls \
    -hls_time 2 \
    -hls_playlist_type vod \
    -hls_list_size 0 \
    -hls_segment_type fmp4 \
    -hls_fmp4_init_filename init.m4s \
    -hls_segment_options movflags=+frag_keyframe+empty_moov+default_base_moof \
    -hls_segment_filename "${outdir}/seg_%d.m4s" \
    "${outdir}/stream.m3u8"
done

echo "Done. Output in ${hls}/{tiger,jellyfish,sintel}/"
