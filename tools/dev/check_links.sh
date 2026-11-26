#!/usr/bin/env bash
# Check that the relative links in the user-facing markdown resolve to files in the repo.
# Why: the help pages are linked from several places and the site renders docs/help, so a
# moved or renamed page would leave dead links that nothing else catches.
# Checks README.md, AGENTS.md, docs/help/*.md and docs/vox/README.md. Skips http(s):, mailto:
# and pure #anchor links; strips #fragments. Exits 1 and prints BROKEN lines if any path is missing.
# Usage: tools/dev/check_links.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

files=(README.md AGENTS.md docs/vox/README.md docs/help/*.md)
broken=0

for file in "${files[@]}"; do
  dir=$(dirname "$file")
  # every "](target)" on a line; grep exits 1 when a file has no links, hence the "|| true"
  targets=$(grep -o '](\([^)]*\))' "$file" | sed -e 's/^](//' -e 's/)$//' -e 's/ .*$//' || true)
  while IFS= read -r target; do
    [[ -z "$target" ]] && continue
    case "$target" in
      http://*|https://*|mailto:*|\#*) continue ;;
    esac
    path="${target%%#*}"
    [[ -z "$path" ]] && continue
    if [[ "$path" == /* ]]; then
      resolved=".$path"
    else
      resolved="$dir/$path"
    fi
    if [[ ! -e "$resolved" ]]; then
      echo "BROKEN: $file -> $target"
      broken=$((broken + 1))
    fi
  done <<< "$targets"
done

if [[ $broken -gt 0 ]]; then
  echo "$broken broken link(s)" >&2
  exit 1
fi
echo "all links ok"
