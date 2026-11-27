#!/usr/bin/env bash
# Sets up vox.fail's Cloudflare side with the cf command line (signed in with `cf auth login`), for
# the servers in a Cloudflare Container behind the edge Worker (docs/design/cloudflare-hosting.md):
# the zone settings, the cache and www redirect rules, the DNS records, the R2 bucket the
# databases are streamed to, and the Worker's secrets. It also takes away the old tunnel "vox".
# Safe to run again: each step checks what's there first, a secret already set is left as it is
# (a new directory key would sign everyone out), and it stops rather than replace a DNS record it
# doesn't expect.
#
#   deploy/cloudflare-setup.sh
#   VOX_DISCORD_CLIENT_ID=… VOX_DISCORD_CLIENT_SECRET=… VOX_STEAM_KEY=… deploy/cloudflare-setup.sh
#                         also sets (or replaces) the sign-in sites' secrets
#
# The Worker (apps/edge, vox-edge) must exist: secrets belong to it. Secrets go to Cloudflare from
# a private temporary file, never a command line, and are never printed.
set -euo pipefail

ZONE_NAME=vox.fail
TUNNEL_NAME=vox # retired: deleted if it's still there
WORKER=vox-edge
BUCKET=vox-data
CF="${CF:-cf}"
command -v "$CF" >/dev/null || CF="$HOME/.bun/bin/cf"
command -v "$CF" >/dev/null || { echo "needs the cf command line"; exit 1; }
cd /tmp # cf reads the nearest cloudflare.config.ts; this script needs none

log() { echo "cloudflare-setup: $*"; }

# cf prints an optional "result_info: ..." line, then JSON. Evaluate a Python expression on it
# (always a fixed expression written in this script, never text from the API).
q() {
  python3 -I -c '
import json, sys
raw = "\n".join(l for l in sys.stdin.read().splitlines() if not l.startswith("result_info:"))
i = min([x for x in (raw.find("{"), raw.find("[")) if x >= 0] or [0])
d = json.loads(raw[i:])
rows = d.get("result", d) if isinstance(d, dict) else d
print(eval(sys.argv[1]))' "$1"
}

ZONE_ID="$("$CF" zones list --name "$ZONE_NAME" | q 'rows[0]["id"]')"
ACCOUNT_ID="$("$CF" zones list --name "$ZONE_NAME" | q 'rows[0]["account"]["id"]')"
export CLOUDFLARE_ACCOUNT_ID="$ACCOUNT_ID"
log "zone $ZONE_NAME"

# ---- zone settings (deploy/README.md, "In the Cloudflare dashboard")
setting() {
  "$CF" zones settings edit "$1" --zone "$ZONE_NAME" --body "{\"value\":\"$2\"}" >/dev/null
  log "setting $1 = $2"
}
setting always_use_https on
setting websockets on
setting rocket_loader off      # rewrites the pages' scripts, which their content policy blocks
setting email_obfuscation off  # same

# ---- rules: cache the game's big files; www.vox.fail -> vox.fail
"$CF" rulesets entrypoint update http_request_cache_settings --zone "$ZONE_NAME" --body '{
  "rules": [{
    "description": "Cache the browser client and game files (the directory marks them five minutes)",
    "expression": "(starts_with(http.request.uri.path, \"/pkg/\") or starts_with(http.request.uri.path, \"/assets/\") or starts_with(http.request.uri.path, \"/data/\"))",
    "action": "set_cache_settings",
    "action_parameters": {"cache": true, "edge_ttl": {"mode": "respect_origin"}}
  }]}' >/dev/null
log "cache rule: /pkg/, /assets/, /data/"
"$CF" rulesets entrypoint update http_request_dynamic_redirect --zone "$ZONE_NAME" --body '{
  "rules": [{
    "description": "www.vox.fail to vox.fail",
    "expression": "(http.host eq \"www.vox.fail\")",
    "action": "redirect",
    "action_parameters": {"from_value": {"status_code": 301, "preserve_query_string": true,
      "target_url": {"expression": "concat(\"https://vox.fail\", http.request.uri.path)"}}}
  }]}' >/dev/null
log "redirect rule: www.$ZONE_NAME -> $ZONE_NAME"

# ---- the old tunnel: the Worker answers everything now
TUNNEL_ID="$("$CF" tunnels list --name "$TUNNEL_NAME" --is-deleted false | q 'rows[0]["id"] if rows else ""')"

# ---- DNS: vox.fail and www.vox.fail point nowhere, proxied: the Worker on vox.fail/* answers
# everything, and the redirect rule answers www
record() { # name -> "type content id" of its one record, "" for none, MANY for more
  "$CF" dns records list --zone "$ZONE_NAME" --name-exact "$1" \
    | q '"" if not rows else (" ".join([rows[0]["type"], rows[0]["content"], rows[0]["id"]]) if len(rows) == 1 else "MANY")'
}
placeholder() { # name comment
  "$CF" dns records create --zone "$ZONE_NAME" --body "{\"type\":\"AAAA\",\"name\":\"$1\",\"content\":\"100::\",\"proxied\":true,\"ttl\":1,\"comment\":\"$2\"}" >/dev/null
}
read -r type content id <<<"$(record "$ZONE_NAME")" || true
if [ "${type:-}" = AAAA ] && [ "$content" = "100::" ]; then
  log "$ZONE_NAME already points at the Worker"
else
  if [ "${type:-}" = CNAME ] && [ -n "$TUNNEL_ID" ] && [ "$content" = "$TUNNEL_ID.cfargotunnel.com" ]; then
    "$CF" dns records delete "$id" --zone "$ZONE_NAME" --force >/dev/null
    log "removed $ZONE_NAME -> tunnel $TUNNEL_NAME"
  elif [ -n "${type:-}" ]; then
    log "$ZONE_NAME has another record (${type} ${content}); not touching it"; exit 1
  fi
  placeholder "$ZONE_NAME" "For the vox-edge Worker, which answers vox.fail"
  log "$ZONE_NAME -> proxied placeholder (the Worker answers)"
fi
if [ -n "$TUNNEL_ID" ]; then
  "$CF" tunnels delete "$TUNNEL_ID" --force >/dev/null
  log "deleted the tunnel $TUNNEL_NAME ($TUNNEL_ID)"
fi
read -r type content id <<<"$(record "www.$ZONE_NAME")" || true
if [ -z "${type:-}" ]; then
  placeholder "www.$ZONE_NAME" "Only for the www redirect rule"
  log "www.$ZONE_NAME -> proxied placeholder (for the redirect)"
else
  log "www.$ZONE_NAME has a record already ($type $content)"
fi

# ---- R2: where Litestream streams the databases
if [ "$("$CF" r2 buckets list | q '[b["name"] for b in (rows.get("buckets", []) if isinstance(rows, dict) else rows)]' | grep -c "'$BUCKET'")" = 0 ]; then
  "$CF" r2 buckets create --body "{\"name\":\"$BUCKET\"}" >/dev/null
  log "R2 bucket $BUCKET created"
else
  log "R2 bucket $BUCKET exists"
fi

# ---- the Worker's secrets (read by the container, apps/edge/src/container.ts)
HAVE="$("$CF" workers secrets list --worker "$WORKER" | q '" ".join(r["name"] for r in rows)')"
has() { [[ " $HAVE " == *" $1 "* ]]; }
put() { # name; the value on stdin, sent from a private file and removed
  python3 -I -c 'import json, sys; print(json.dumps({"name": sys.argv[1], "text": sys.stdin.read().strip(), "type": "secret_text"}))' "$1" \
    > "$TMP/body.json"
  "$CF" workers secrets update "$1" --worker "$WORKER" --body "@$TMP/body.json" >/dev/null
  rm -f "$TMP/body.json"
  log "secret $1 set"
}
# one private folder for every temporary file, removed however the script ends
TMP="$(umask 077 && mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
(umask 077
  has VOX_DIRECTORY_KEY || openssl rand -hex 32 | put VOX_DIRECTORY_KEY
  has VOX_WORLD_SECRET || openssl rand -hex 32 | put VOX_WORLD_SECRET
  for s in VOX_DISCORD_CLIENT_ID VOX_DISCORD_CLIENT_SECRET VOX_STEAM_KEY; do
    if [ -n "${!s:-}" ]; then printf '%s' "${!s}" | put "$s"; elif ! has "$s"; then log "$s isn't set: pass it in the environment to set it"; fi
  done
  # Litestream's R2 keys: a token that can read and write this bucket only. Its id is the S3
  # access key and the SHA-256 of its value the secret key.
  if ! has LITESTREAM_ACCESS_KEY_ID || ! has LITESTREAM_SECRET_ACCESS_KEY; then
    body='{"name":"vox-data (Litestream in the vox-edge container)","policies":[{"effect":"allow","resources":{"com.cloudflare.edge.r2.bucket.'"$ACCOUNT_ID"'_default_'"$BUCKET"'":"*"},"permission_groups":[{"id":"2efd5506f9c8494dacb1fa10a3e7d5b6"},{"id":"6a018a9f2fc74eb6b293b0c548f38b39"}]}]}'
    # both keys are in hand before either is sent, so a failure can't leave one set without
    # the other (a re-run would make a second token)
    keys="$TMP/r2-keys"
    "$CF" accounts tokens create --body "$body" | python3 -I -c '
import hashlib, json, sys
raw = sys.stdin.read(); d = json.loads(raw[raw.find("{"):]); r = d.get("result") or d
if not (r.get("id") and r.get("value")): sys.exit("cloudflare-setup: making the R2 token failed")
print(r["id"]); print(hashlib.sha256(r["value"].encode()).hexdigest())' > "$keys"
    [ "$(wc -l < "$keys")" = 2 ] || { log "making the R2 token gave no keys"; exit 1; }
    sed -n 2p "$keys" | put LITESTREAM_SECRET_ACCESS_KEY
    sed -n 1p "$keys" | put LITESTREAM_ACCESS_KEY_ID
    log "R2 token for $BUCKET made (Workers R2 Storage Bucket Item Read and Write on it only)"
  fi
)

log "done"
