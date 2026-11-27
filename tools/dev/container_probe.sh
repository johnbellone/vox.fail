#!/usr/bin/env bash
# Runs vox.fail's container start (deploy/docker/start.sh, vox-supervise, litestream.yml and the
# production args files) with the real servers against an R2 bucket, on an empty folder each time
# as a Cloudflare container gets, and checks an account survives a restart:
#
#   1. start; sign up an account; note the directory's public key; stop (SIGTERM)
#   2. empty disk: start; directory.db and the worlds are restored from R2; the account signs in
#      with its password and the public key is the same (from VOX_DIRECTORY_KEY)
#
#   tools/dev/container_probe.sh <env file> [litestream binary]
#
# The env file holds VOX_R2_BUCKET, VOX_R2_ENDPOINT, LITESTREAM_ACCESS_KEY_ID and
# LITESTREAM_SECRET_ACCESS_KEY (use a test bucket). Uses target/release/{vox-directory,vox-host}
# (cargo build --release -p vox-directory -p vox-host), ports 17700 and 17790, and writes under
# probe/<time>/ in the bucket. Prints PASS or FAIL.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ENV_FILE="${1:?usage: container_probe.sh <env file> [litestream binary]}"
LITESTREAM="${2:-$(command -v litestream || true)}"
[ -x "$LITESTREAM" ] || { echo "needs the litestream binary"; exit 1; }
for b in vox-directory vox-host; do
  [ -x "$ROOT/target/release/$b" ] || { echo "build first: cargo build --release -p vox-directory -p vox-host"; exit 1; }
done
set -a; . "$ENV_FILE"; set +a

WORK="$(mktemp -d)"
trap 'kill -9 $(jobs -p) 2>/dev/null; rm -rf "$WORK"' EXIT
DATA="$WORK/data"
mkdir -p "$WORK/bin" "$WORK/etc"
for b in vox-directory vox-host; do ln -s "$ROOT/target/release/$b" "$WORK/bin/$b"; done
ln -s "$(readlink -f "$LITESTREAM")" "$WORK/bin/litestream"
ln -s "$ROOT/deploy/docker/vox-supervise" "$WORK/bin/vox-supervise"
cp "$ROOT/deploy/docker/litestream.yml" "$WORK/etc/"
# production's args, moved to local ports and the test folder
for f in directory.args host.args; do
  sed -e "s|0.0.0.0:7700|127.0.0.1:17700|; s|0.0.0.0:7790|127.0.0.1:17790|" \
      -e "s|http://127.0.0.1:7700|http://127.0.0.1:17700|; s|http://127.0.0.1:7790|http://127.0.0.1:17790|" \
      -e "s|https://vox.fail|http://127.0.0.1:17700|; s|/var/lib/vox|$DATA|g" \
      "$ROOT/deploy/docker/$f" > "$WORK/etc/$f"
done

export VOX_ETC="$WORK/etc" VOX_BIN="$WORK/bin" VOX_DATA="$DATA"
export VOX_R2_PREFIX="probe/$(date +%s)"
export VOX_DIRECTORY_KEY="$(openssl rand -hex 32)"
export VOX_WORLD_SECRET="$(openssl rand -hex 32)"
D=http://127.0.0.1:17700 H=http://127.0.0.1:17790
NAME="probe$(date +%s)" PASS="probe-$(openssl rand -hex 8)"
fail() { echo "FAIL: $*"; echo "--- log"; cat "$WORK/log"; exit 1; }

start() {
  rm -rf "$DATA"
  bash "$ROOT/deploy/docker/start.sh" > "$WORK/log" 2>&1 &
  PID=$!
  for _ in $(seq 240); do
    curl -fsS -o /dev/null "$D/api/me" 2>/dev/null && curl -fsS -o /dev/null "$H/" 2>/dev/null && return 0
    kill -0 "$PID" 2>/dev/null || fail "start.sh stopped"
    sleep 0.25
  done
  fail "the servers didn't answer within a minute"
}
stop() {
  sleep 3 # Litestream ships within a second
  kill -TERM "$PID"
  for _ in $(seq 120); do kill -0 "$PID" 2>/dev/null || break; sleep 0.25; done
  kill -0 "$PID" 2>/dev/null && fail "still running 30 s after SIGTERM"
  wait "$PID" 2>/dev/null || true
}
post() { curl -sS -o "$WORK/out" -w '%{http_code}' -H 'Content-Type: application/json' -d "$2" "$D$1"; }
login='{"name":"'"$NAME"'","password":"'"$PASS"'"}'

start
code="$(post /api/signup "$login")"
[ "$code" = 200 ] || fail "sign-up answered $code: $(cat "$WORK/out")"
KEY1="$(curl -fsS "$D/api/key")"
curl -fsS "$H/w/void/" >/dev/null || fail "vox-host doesn't serve the Void"
stop
grep -q 'restored directory.db' "$WORK/log" && fail "round 1 found a copy in R2 under a fresh prefix"
echo "round 1: signed up $NAME, stopped cleanly"

start
grep -q 'restored directory.db' "$WORK/log" || fail "round 2 didn't restore directory.db"
code="$(post /api/signin "$login")"
[ "$code" = 200 ] || fail "sign-in after the restart answered $code: $(cat "$WORK/out")"
KEY2="$(curl -fsS "$D/api/key")"
[ "$KEY1" = "$KEY2" ] || fail "the directory's key changed across the restart"
stop
echo "round 2: restored from R2, $NAME signed in, same key ($(grep -c '^start: restored' "$WORK/log") databases restored)"
echo PASS
