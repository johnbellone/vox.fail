#!/usr/bin/env bash
# Checks that vox.fail's container keeps its data across restarts (docs/design/cloudflare-hosting.md):
# runs the real deploy/docker/start.sh and litestream.yml against an R2 bucket, with a small
# SQLite writer standing in for the servers, on an empty folder each time, as a container gets.
#
#   1. start, write rows, stop cleanly (SIGTERM)
#   2. empty disk: start, all rows restored; write more, then kill everything (SIGKILL)
#   3. empty disk: start, the rows written before the kill are there too
#   4. a wrong R2 secret: start.sh stops with an error and nothing starts, rather than begin
#      with an empty database and overwrite the copy in R2
#
#   tools/dev/litestream_probe.sh <env file> [litestream binary]
#
# The env file holds VOX_R2_BUCKET, VOX_R2_ENDPOINT, LITESTREAM_ACCESS_KEY_ID and
# LITESTREAM_SECRET_ACCESS_KEY (use a test bucket). Each run writes under probe/<time>/.
# Needs sqlite3. Prints PASS or FAIL.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ENV_FILE="${1:?usage: litestream_probe.sh <env file> [litestream binary]}"
LITESTREAM="${2:-$(command -v litestream || true)}"
[ -x "$LITESTREAM" ] || { echo "needs the litestream binary"; exit 1; }
command -v sqlite3 >/dev/null || { echo "needs sqlite3"; exit 1; }
set -a; . "$ENV_FILE"; set +a

WORK="$(mktemp -d)"
trap 'kill -9 $(jobs -p) 2>/dev/null; rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin" "$WORK/etc"
ln -s "$(readlink -f "$LITESTREAM")" "$WORK/bin/litestream"
cp "$ROOT/deploy/docker/litestream.yml" "$WORK/etc/"
printf -- '--world\nprobe@1\n' > "$WORK/etc/host.args" # start.sh restores probe.db

# the stand-in for the servers: add rows, say so, then wait to be stopped
cat > "$WORK/bin/writer" <<'EOF'
#!/usr/bin/env bash
db="$VOX_DATA/probe.db"
sqlite3 "$db" "PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS t (n INTEGER);" >/dev/null
for i in $(seq "$FROM" "$TO"); do sqlite3 "$db" "INSERT INTO t VALUES ($i);"; done
touch "$VOX_DATA/written"
trap 'exit 0' TERM INT
while :; do sleep 0.2; done
EOF
chmod +x "$WORK/bin/writer"

export VOX_ETC="$WORK/etc" VOX_BIN="$WORK/bin" VOX_EXEC="$WORK/bin/writer" VOX_SKIP_KEY=1
export VOX_R2_PREFIX="probe/$(date +%s)"
fail() { echo "FAIL: $*"; exit 1; }

# one container's life: start on an empty disk, write FROM..TO, then stop with SIGNAL
round() {
  local from="$1" to="$2" signal="$3" pid
  export VOX_DATA="$WORK/data" FROM="$from" TO="$to"
  rm -rf "$VOX_DATA"
  setsid bash "$ROOT/deploy/docker/start.sh" > "$WORK/log" 2>&1 &
  pid=$!
  for _ in $(seq 120); do [ -e "$VOX_DATA/written" ] && break; sleep 0.25; done
  [ -e "$VOX_DATA/written" ] || { cat "$WORK/log"; fail "the writer didn't finish"; }
  RESTORED_BEFORE="$(grep -c 'restored probe.db' "$WORK/log" || true)"
  sleep 3 # Litestream ships within a second
  if [ "$signal" = KILL ]; then
    kill -KILL -- "-$pid" # the whole group: Litestream and the writer, no last sync
  else
    kill -TERM "$pid"
  fi
  wait "$pid" 2>/dev/null || true
}
count() { sqlite3 "$WORK/data/probe.db" "SELECT count(*), coalesce(max(n), 0) FROM t;"; }

round 1 50 TERM
[ "$(count)" = "50|50" ] || fail "round 1 wrote $(count)"
echo "round 1: wrote 50 rows, stopped cleanly"

round 51 80 KILL
[ "$RESTORED_BEFORE" = 1 ] || { cat "$WORK/log"; fail "round 2 didn't restore probe.db"; }
[ "$(count)" = "80|80" ] || fail "round 2 has $(count) rows (wanted 80|80)"
echo "round 2: restored 50 rows from R2, wrote 30 more, then killed"

round 81 81 TERM
[ "$RESTORED_BEFORE" = 1 ] || { cat "$WORK/log"; fail "round 3 didn't restore probe.db"; }
[ "$(count)" = "81|81" ] || fail "round 3 has $(count) rows (wanted 81|81: the 30 before the kill lost?)"
echo "round 3: restored all 80 rows after the kill"

rm -rf "$WORK/data"
if LITESTREAM_SECRET_ACCESS_KEY=wrong timeout 60 bash "$ROOT/deploy/docker/start.sh" > "$WORK/log" 2>&1; then
  cat "$WORK/log"; fail "start.sh went on with a wrong secret"
fi
[ -e "$WORK/data/written" ] && fail "the servers started with a wrong secret"
grep -q 'from R2 failed' "$WORK/log" || { cat "$WORK/log"; fail "no clear error for a wrong secret"; }
echo "round 4: a wrong secret stops the start"
echo PASS
