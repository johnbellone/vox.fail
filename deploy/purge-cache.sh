#!/usr/bin/env bash
# After a release: drop Cloudflare's copies of the browser client and game files (it keeps
# them up to five minutes otherwise).
#
#   CF_ZONE=<zone id> CF_TOKEN=<API token with Cache Purge> deploy/purge-cache.sh
set -euo pipefail
curl -fsS -X POST "https://api.cloudflare.com/client/v4/zones/${CF_ZONE:?zone id}/purge_cache" \
  -H "Authorization: Bearer ${CF_TOKEN:?API token}" \
  -H "Content-Type: application/json" \
  --data '{"purge_everything":true}'
echo
