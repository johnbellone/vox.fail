#!/usr/bin/env bash
# Sets up vox.fail's Cloudflare side with the cf command line (signed in with `cf auth login`):
# the zone settings, the cache and www redirect rules, the tunnel "vox" (managed in Cloudflare,
# so the server needs only its token) and the DNS records. Safe to run again: each step checks
# what's there first, and it stops rather than replace a DNS record it doesn't expect.
#
#   deploy/cloudflare-setup.sh
#
# The tunnel sends /w/... to vox-host (127.0.0.1:7790, every world), refuses /api/world/..., and
# sends the rest to vox-directory (127.0.0.1:7700). With no server connected the tunnel answers
# error 1033 and the edge Worker (apps/edge) shows the offline page. Then, on the server:
#   sudo cloudflared service install "$(cf tunnels token get <tunnel id>)"
set -euo pipefail

ZONE_NAME=vox.fail
TUNNEL_NAME=vox
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

# ---- the tunnel, managed in Cloudflare
TUNNEL_ID="$("$CF" tunnels list --name "$TUNNEL_NAME" --is-deleted false | q 'rows[0]["id"] if rows else ""')"
if [ -z "$TUNNEL_ID" ]; then
  TUNNEL_ID="$("$CF" tunnels create --body "{\"name\":\"$TUNNEL_NAME\",\"config_src\":\"cloudflare\"}" | q 'rows["id"]')"
  log "tunnel $TUNNEL_NAME created ($TUNNEL_ID)"
else
  log "tunnel $TUNNEL_NAME exists ($TUNNEL_ID)"
fi
"$CF" tunnels config update "$TUNNEL_ID" --body "{\"config\":{\"ingress\":[
  {\"hostname\":\"$ZONE_NAME\",\"path\":\"^/w/\",\"service\":\"http://127.0.0.1:7790\"},
  {\"hostname\":\"$ZONE_NAME\",\"path\":\"^/api/world/\",\"service\":\"http_status:404\"},
  {\"hostname\":\"$ZONE_NAME\",\"service\":\"http://127.0.0.1:7700\"},
  {\"service\":\"http_status:404\"}]}}" >/dev/null
log "tunnel routes: /w/ -> vox-host :7790, /api/world/ refused, the rest -> vox-directory :7700"

# ---- DNS: vox.fail -> the tunnel; www.vox.fail -> anywhere proxied (the redirect answers)
record() { # name -> "type content" of its one record, or "" (more than one: stop)
  "$CF" dns records list --zone "$ZONE_NAME" --name-exact "$1" \
    | q '"" if not rows else (" ".join([rows[0]["type"], rows[0]["content"], rows[0]["id"]]) if len(rows) == 1 else "MANY")'
}
TARGET="$TUNNEL_ID.cfargotunnel.com"
read -r type content id <<<"$(record "$ZONE_NAME")" || true
if [ "${type:-}" = CNAME ] && [ "$content" = "$TARGET" ]; then
  log "$ZONE_NAME already points at the tunnel"
else
  if [ "${type:-}" = AAAA ] && [ "$content" = "100::" ]; then
    "$CF" dns records delete "$id" --zone "$ZONE_NAME" --force >/dev/null
    log "removed the placeholder AAAA $ZONE_NAME 100::"
  elif [ -n "${type:-}" ]; then
    log "$ZONE_NAME has another record (${type} ${content}); not touching it"; exit 1
  fi
  "$CF" dns records create --zone "$ZONE_NAME" --body "{\"type\":\"CNAME\",\"name\":\"$ZONE_NAME\",\"content\":\"$TARGET\",\"proxied\":true,\"ttl\":1,\"comment\":\"Tunnel vox (deploy/cloudflare-setup.sh)\"}" >/dev/null
  log "$ZONE_NAME -> tunnel $TUNNEL_NAME"
fi
read -r type content id <<<"$(record "www.$ZONE_NAME")" || true
if [ -z "${type:-}" ]; then
  "$CF" dns records create --zone "$ZONE_NAME" --body "{\"type\":\"AAAA\",\"name\":\"www.$ZONE_NAME\",\"content\":\"100::\",\"proxied\":true,\"ttl\":1,\"comment\":\"Only for the www redirect rule\"}" >/dev/null
  log "www.$ZONE_NAME -> proxied placeholder (for the redirect)"
else
  log "www.$ZONE_NAME has a record already ($type $content)"
fi

log "done. On the server: sudo cloudflared service install \"\$(cf tunnels token get $TUNNEL_ID)\""
