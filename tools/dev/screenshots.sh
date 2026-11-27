#!/usr/bin/env bash
# The README's pictures: each world from a set view, drawn by the real client (no HUD), one
# PNG per shot into docs/screenshots/, which the README shows. Run it when the look of the game
# has changed, then commit the pictures with the change, so the README keeps up.
#
# Usage: tools/dev/screenshots.sh [out folder, docs/screenshots] [shot names...]
#   e.g. tools/dev/screenshots.sh docs/screenshots sea      (just the sea)
#
# Each shot is a fresh world from its level file (a scratch database, thrown away), the
# client's screenshot mode (bins/vox-client/src/lib.rs, `screenshot`) and the control page's
# `world:` commands for the view. Without a GPU, run it under Mesa's software Vulkan the way
# the visual tests do (see .github/workflows/visual.yml).
set -euo pipefail
cd "$(dirname "$0")/../.."

out="${1:-docs/screenshots}"
shift || true
want=" $* "

# name | level | seconds after the world is drawn | commands, `;` between them
shots=(
  "void|void|3|goto:0:0:-22;look:0:0"                      # a portal: the yard on its far side
  "sea|sea|3|time:20.5"                                    # the pier's fires over the swell
  "islands|islands|3|look:90:-5"                           # a doorway among the floating islands
  "yard|yard|3|time:10;camera:behind;zoom:9"               # a character at home
  "orbit|orbit|3|goto:80:0:2;look:0:-35"                   # the planet from the station's truss
)

# SHOTS (one shot a line, in the same form) replaces the list: for trying out views
if [[ -n "${SHOTS:-}" ]]; then
  mapfile -t shots <<<"$SHOTS"
fi

cargo build --release -p vox-server -p vox-client
mkdir -p "$out"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

port=7799
for shot in "${shots[@]}"; do
  IFS='|' read -r name level after cmds <<<"$shot"
  if [[ "$want" != "  " && "$want" != *" $name "* ]]; then
    continue
  fi
  port=$((port + 1))
  ./target/release/vox-server --listen "127.0.0.1:$port" --name "$level" --db "$scratch/$name.db" \
    --seed "data/levels/$level.json" --no-snapshots --trust-moves >"$scratch/$name.server.log" 2>&1 &
  server=$!
  sleep 1
  echo "shot $name ($level)"
  if ! VOX_SCREENSHOT="$out/$name.png" VOX_SCREENSHOT_AFTER="$after" VOX_SCREENSHOT_DO="$cmds" VOX_SCREENSHOT_CLEAN=1 VOX_SCREENSHOT_SIZE=960x540 \
    timeout 600 ./target/release/vox-client --world "ws://127.0.0.1:$port/ws" --name Visitor >"$scratch/$name.client.log" 2>&1; then
    echo "  the client failed; its last lines:" >&2
    tail -n 20 "$scratch/$name.client.log" >&2
  fi
  kill -INT "$server" 2>/dev/null || true
  wait "$server" 2>/dev/null || true
  [[ -s "$out/$name.png" ]] && echo "  $out/$name.png" || echo "  no picture for $name" >&2
done
