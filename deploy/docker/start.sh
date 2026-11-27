#!/usr/bin/env bash
# The container's start on Cloudflare (docs/design/cloudflare-hosting.md). The disk starts empty
# every time, so:
#   1. write the directory's signing key from VOX_DIRECTORY_KEY (the same key every start, or
#      every sign-in token would stop working);
#   2. restore each database from R2 (the directory's and each world's), when R2 has one;
#   3. run Litestream, which streams every change back to R2, with vox-supervise (the directory
#      and vox-host) as its child: when they stop, Litestream syncs once more and stops too.
#
# Needs: VOX_DIRECTORY_KEY, VOX_R2_BUCKET, VOX_R2_ENDPOINT, LITESTREAM_ACCESS_KEY_ID and
# LITESTREAM_SECRET_ACCESS_KEY; the servers' own secrets (VOX_WORLD_SECRET, the Discord and
# Steam ones) pass through. The flags are in /opt/vox/etc/{directory,host}.args.
set -euo pipefail

export VOX_DATA="${VOX_DATA:-/var/lib/vox}"
export VOX_R2_PREFIX="${VOX_R2_PREFIX:-vox}"
ETC="${VOX_ETC:-/opt/vox/etc}"
BIN="${VOX_BIN:-/opt/vox/bin}"
CONFIG="$ETC/litestream.yml"

say() { echo "start: $*"; }
die() { echo "start: $*" >&2; exit 1; }

for v in VOX_R2_BUCKET VOX_R2_ENDPOINT LITESTREAM_ACCESS_KEY_ID LITESTREAM_SECRET_ACCESS_KEY; do
  [ -n "${!v:-}" ] || die "$v isn't set"
done
[[ "$VOX_R2_BUCKET" =~ ^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$ ]] || die "VOX_R2_BUCKET isn't a bucket name"
[[ "$VOX_R2_PREFIX" =~ ^[A-Za-z0-9._-][A-Za-z0-9._/-]{0,99}$ ]] && [[ "$VOX_R2_PREFIX" != *..* ]] \
  || die "VOX_R2_PREFIX may hold letters, digits and . _ / - only"
[[ "$VOX_R2_ENDPOINT" =~ ^https://[A-Za-z0-9.-]+$ ]] || die "VOX_R2_ENDPOINT must be https://<host>"
mkdir -p "$VOX_DATA"

# 1. the signing key: the file's exact text, 64 hex digits and a newline
if [ -z "${VOX_SKIP_KEY:-}" ]; then
  key="$(printf '%s' "${VOX_DIRECTORY_KEY:-}" | tr -d '[:space:]')"
  [[ "$key" =~ ^[0-9a-f]{64}$ ]] || die "VOX_DIRECTORY_KEY must be the key file's 64 hex digits"
  (umask 077 && printf '%s\n' "$key" > "$VOX_DATA/directory.key")
  unset VOX_DIRECTORY_KEY key # the servers don't need it in their environment
fi

# 2. restore: the directory, and each world named in host.args (--world <id>@<port>)
names=(directory)
if [ -f "$ETC/host.args" ]; then
  while IFS= read -r line; do
    [[ "$line" =~ ^[a-z0-9-]+@[0-9]+$ ]] && names+=("${line%@*}")
  done < "$ETC/host.args"
fi
# more databases to restore by name (tests). Only top-level <name>.db: Litestream also
# replicates folders (atlas tiles, atlas-<name>/<i>-<j>.db), which an atlas on vox.fail would
# have to restore too.
read -ra extra <<<"${VOX_RESTORE_EXTRA:-}"
names+=("${extra[@]}")
for n in "${names[@]}"; do
  [[ "$n" =~ ^[A-Za-z0-9_-]+$ ]] || die "not a database name: $n"
  db="$VOX_DATA/$n.db"
  if [ -e "$db" ]; then
    say "$n.db is here already; not restoring"
    continue
  fi
  "$BIN/litestream" restore -if-replica-exists -o "$db" \
    "s3://$VOX_R2_BUCKET/$VOX_R2_PREFIX/$n.db?endpoint=$VOX_R2_ENDPOINT" \
    || die "restoring $n.db from R2 failed"
  if [ -e "$db" ]; then say "restored $n.db"; else say "no copy of $n.db in R2 yet: it starts new"; fi
done

# 3. replicate, with the servers as Litestream's child
exec "$BIN/litestream" replicate -config "$CONFIG" -exec "${VOX_EXEC:-$BIN/vox-supervise}"
