#!/usr/bin/env bash
# Checks code comments against the rules in CONTRIBUTING.md ("Never in a comment" and
# "Terminology"): no milestone numbers, audit IDs, dates or references to external documents,
# and "transition" rather than "morph".
#
# Usage: scripts/check-comments.sh [PATH...]    (default: crates examples)
#
# Scans .rs, .ts and .wgsl files. Only comment text is checked: `//` comments, and `*` lines
# inside TypeScript block comments. A line containing `check-comments: allow` is skipped.
# Exits 1 if anything is found.

set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$#" -eq 0 ]; then
  set -- crates examples
fi

# name|grep flags|pattern
rules=(
  'milestone reference|-E|\bM[0-9]+(\.[0-9]+)?\b'
  'audit ID|-E|\b[ACKXDTR]-[0-9]{2}\b'
  'audit reference|-iE|\baudit(ed|s)?\b'
  'date|-E|\b20[0-9]{2}-[0-9]{2}(-[0-9]{2})?\b'
  'external document|-E|\b[A-Z][A-Z_]+\.md\b|\bPLANNING\b|\bROADMAP\b|\bPhase [A-E]\b'
  'say "transition", not "morph"|-iE|\bmorph'
)

files=$(find "$@" -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.wgsl' \) \
  -not -path '*/node_modules/*' -not -path '*/target/*' -not -path '*/dist/*' \
  -not -path '*/pkg/*' -not -path '*/pkg-host/*' -not -name '*.d.ts' | sort)

found=0
for file in $files; do
  # Print "LINE:comment text" for every comment on a line, dropping the code before it.
  comments=$(awk '
    /check-comments: allow/ { next }
    {
      i = index($0, "//")
      if (i > 0) { text = substr($0, i + 2); sub(/^[\/!]+/, "", text); print NR ":" text; next }
      if ($0 ~ /^[ \t]*\*( |$)/ || $0 ~ /^[ \t]*\/\*/) { print NR ":" $0 }
    }' "$file")
  [ -z "$comments" ] && continue

  for rule in "${rules[@]}"; do
    name=${rule%%|*}
    rest=${rule#*|}
    flags=${rest%%|*}
    pattern=${rest#*|}
    while IFS= read -r hit; do
      [ -z "$hit" ] && continue
      echo "$file:${hit%%:*}: $name:${hit#*:}"
      found=$((found + 1))
    done < <(printf '%s\n' "$comments" | grep $flags -- "$pattern" || true)
  done
done

if [ "$found" -gt 0 ]; then
  echo
  echo "check-comments: $found problem(s). See CONTRIBUTING.md, \"Writing comments and docs\"."
  exit 1
fi
echo "check-comments: ok"
