#!/usr/bin/env bash
# Compare the plain snapshot store with Oxen (docs/design/world-format.md, *Oxen*): the same
# world and the same rounds of building, committed both ways; prints sizes and times.
#
#   tools/dev/oxen_probe.sh <out folder> [tiles 4] [rounds 20] [zones per round 3]
#
# Needs `oxen` on the PATH (github.com/Oxen-AI/Oxen releases). Run from the repo root.
set -euo pipefail
out=${1:?an output folder}
tiles=${2:-4} rounds=${3:-20} per=${4:-3}
O=${OXEN:-oxen}
ms() { date +%s%3N; }

cargo run --release -q -p vox-format --example oxen_probe -- "$out" "$tiles" "$rounds" "$per"
echo "plain store: $(du -sk "$out/plain" | cut -f1) KB"

mkdir -p "$out/tree"
cd "$out/tree"
"$O" init >/dev/null
total=0
for i in $(seq 0 "$rounds"); do
  cp "../rounds/$i/"* .
  a=$(ms)
  "$O" add . >/dev/null
  "$O" commit -m "round $i" >/dev/null
  t=$(( $(ms) - a ))
  total=$(( total + t ))
  [ "$i" -eq 0 ] && echo "oxen first commit (whole world): $t ms"
done
echo "oxen commits 1..$rounds: $(( (total) / (rounds + 1) )) ms each on average"
echo "oxen store: $(du -sk .oxen | cut -f1) KB (versions $(du -sk .oxen/versions | cut -f1) KB), working tree $(du -sk --exclude=.oxen . | cut -f1) KB"
first=$("$O" log | grep '^commit' | tail -1 | awk '{print $2}')
a=$(ms); "$O" checkout "$first" >/dev/null; echo "checkout the oldest: $(( $(ms) - a )) ms"
a=$(ms); "$O" checkout main >/dev/null; echo "checkout the newest: $(( $(ms) - a )) ms"
