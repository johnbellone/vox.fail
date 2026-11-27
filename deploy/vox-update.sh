#!/usr/bin/env bash
# Installs a server release of vox.fail from GitHub: the bundle that .github/workflows/release.yml
# attaches to each version tag (vox-linux-x86_64.tar.gz). Run by vox-update.timer as root; the
# server pulls, so nothing outside can write to it and GitHub holds no key to the machine.
#
#   vox-update.sh                 install the latest release, if it isn't the running one
#   vox-update.sh --tag v0.4.0    install that release and hold it (also to go back to an older one)
#   vox-update.sh --rollback      switch back to the release before this one, and hold it
#   vox-update.sh --resume        drop a hold and install the latest release again
#   vox-update.sh --force         reinstall the latest and restart, even if it's running (drops a hold)
#
# A hold keeps the timer from installing newer releases, so a rollback stays rolled back.
#
# Each release is unpacked into /opt/vox-releases/<tag>, and /opt/vox is a symlink to the running
# one, switched in one rename: the systemd units keep using /opt/vox. After switching, the vox
# services that were running are restarted (the directory first; worlds save on SIGTERM) and the
# directory must answer within a minute, or the previous release goes back. Then Cloudflare's
# copies of the browser client and game files are purged, when CF_ZONE and CF_TOKEN are set.
# A release that fails is remembered and not tried again by the timer.
#
# Settings come from /etc/vox/deploy.env (see deploy.env): VOX_REPO, GITHUB_TOKEN (only for a
# private repository), VOX_VERIFY_ATTESTATION, CF_ZONE, CF_TOKEN. Needs curl, jq, tar, flock.
set -euo pipefail

REPO="${VOX_REPO:-johnbellone/vox.fail}"
LINK="${VOX_LINK:-/opt/vox}"
ROOT="${VOX_ROOT:-/opt/vox-releases}"
ASSET=vox-linux-x86_64.tar.gz
KEEP=3
HEALTH_URL="${VOX_HEALTH_URL:-http://127.0.0.1:7700/}"

log() { echo "vox-update: $*"; }
die() { echo "vox-update: $*" >&2; exit 1; }

tag="" rollback=0 force=0 explicit=0 explicit_tag=0
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) tag="${2:?--tag needs a tag}"; explicit=1; explicit_tag=1; shift 2 ;;
    --rollback) rollback=1; explicit=1; shift ;;
    --force) force=1; explicit=1; shift ;;
    --resume) explicit=1; shift ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

[ "$(id -u)" -eq 0 ] || [ -n "${VOX_ROOT:-}" ] || die "run as root"
for c in curl jq tar flock sha256sum systemctl; do command -v "$c" >/dev/null || die "needs $c"; done
[[ "$REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || die "VOX_REPO must be owner/name"

mkdir -p "$ROOT"
exec 9>"$ROOT/.lock"
flock -n 9 || { log "another update is running"; exit 0; }
rm -rf "$ROOT"/.fetch-*

# A tag becomes a directory name: letters, digits, dots, dashes, underscores only.
valid_tag() { [[ "$1" =~ ^v[0-9A-Za-z._-]{1,64}$ ]] && [[ "$1" != *..* ]]; }

if [ -e "$LINK" ] && [ ! -L "$LINK" ]; then
  die "$LINK is a directory, not a symlink. Once, by hand: systemctl stop the vox services, then
  mv $LINK $ROOT/v0-manual && ln -s $ROOT/v0-manual $LINK, and start them again."
fi
current=""
[ -L "$LINK" ] && current="$(basename "$(readlink "$LINK")")"

api() {
  curl -fsSL --proto =https --retry 3 --max-time 600 \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    ${GITHUB_TOKEN:+-H "Authorization: Bearer $GITHUB_TOKEN"} "$@"
}

# Download, check and unpack a release into $ROOT/<tag> (once; kept for rollback).
fetch() {
  local tag="$1" release tmp url sum_url
  [ -f "$ROOT/$tag/.complete" ] && return 0
  release="$(api -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases/tags/$tag")"
  url="$(jq -r --arg n "$ASSET" '.assets[] | select(.name == $n) | .url' <<<"$release")"
  sum_url="$(jq -r --arg n "$ASSET.sha256" '.assets[] | select(.name == $n) | .url' <<<"$release")"
  [ -n "$url" ] && [ -n "$sum_url" ] || die "$tag has no $ASSET (is its release build finished?)"

  tmp="$(mktemp -d "$ROOT/.fetch-XXXXXX")"
  trap 'rm -rf "$tmp"' RETURN
  log "downloading $tag"
  api -H "Accept: application/octet-stream" -o "$tmp/$ASSET" "$url"
  api -H "Accept: application/octet-stream" -o "$tmp/$ASSET.sha256" "$sum_url"
  # the checksum file names the bundle; check only that line, by our own file name
  awk -v n="$ASSET" '$2 == n || $2 == "*" n { print $1 "  " n; found = 1 } END { exit !found }' \
    "$tmp/$ASSET.sha256" > "$tmp/check" || die "$ASSET.sha256 doesn't name $ASSET"
  (cd "$tmp" && sha256sum --quiet -c check) || die "$tag: checksum mismatch"
  if [ "${VOX_VERIFY_ATTESTATION:-0}" = 1 ]; then
    command -v gh >/dev/null || die "VOX_VERIFY_ATTESTATION=1 needs the gh command line"
    GH_TOKEN="${GITHUB_TOKEN:-}" gh attestation verify "$tmp/$ASSET" --repo "$REPO" >/dev/null \
      || die "$tag: build provenance doesn't verify"
    log "$tag: provenance verified"
  fi

  mkdir "$tmp/tree"
  tar -xzf "$tmp/$ASSET" -C "$tmp/tree" --no-same-owner --no-same-permissions
  for b in vox-directory vox-server vox-host; do
    [ -x "$tmp/tree/bin/$b" ] || die "$tag: bundle has no bin/$b"
  done
  [ -f "$tmp/tree/web/index.html" ] || die "$tag: bundle has no web/index.html"
  [ "$(id -u)" -ne 0 ] || chown -R root:root "$tmp/tree"
  chmod -R go-w "$tmp/tree"
  touch "$tmp/tree/.complete"
  rm -rf "${ROOT:?}/$tag"
  mv "$tmp/tree" "$ROOT/$tag"
}

# Point /opt/vox at a release in one rename, so a service never sees half of one.
switch_to() {
  ln -sfn "$ROOT/$1" "$LINK.new"
  mv -Tf "$LINK.new" "$LINK"
}

# The vox services running now: the directory first, then the worlds (vox-host or vox-world@).
running_units() {
  local all
  all="$(systemctl list-units --type=service --state=active --plain --no-legend \
    'vox-directory.service' 'vox-host.service' 'vox-world@*.service' | awk '{ print $1 }')"
  grep -x 'vox-directory.service' <<<"$all" || true
  grep -vx 'vox-directory.service' <<<"$all" || true
}

restart_and_check() {
  local units=("$@") u
  [ ${#units[@]} -gt 0 ] || { log "no vox services running; nothing to restart"; return 0; }
  for u in "${units[@]}"; do
    log "restarting $u"
    systemctl restart "$u" || return 1
  done
  for _ in $(seq 60); do
    curl -fsS -o /dev/null --max-time 2 "$HEALTH_URL" 2>/dev/null && break
    sleep 1
  done
  curl -fsS -o /dev/null --max-time 2 "$HEALTH_URL" || { log "directory doesn't answer at $HEALTH_URL"; return 1; }
  sleep 3
  for u in "${units[@]}"; do
    systemctl is-active --quiet "$u" || { log "$u didn't stay up"; return 1; }
  done
}

purge_cache() {
  if [ -n "${CF_ZONE:-}" ] && [ -n "${CF_TOKEN:-}" ]; then
    curl -fsS --proto =https -o /dev/null -X POST \
      "https://api.cloudflare.com/client/v4/zones/$CF_ZONE/purge_cache" \
      -H "Authorization: Bearer $CF_TOKEN" -H "Content-Type: application/json" \
      --data '{"purge_everything":true}' && log "Cloudflare cache purged" \
      || log "Cloudflare cache purge failed (the edge keeps old files up to five minutes)"
  fi
}

prune() {
  local keep=("$current" "$1") d name
  # newest first by install time; keep $KEEP plus whatever is running or was running
  ls -1dt "$ROOT"/v* 2>/dev/null | tail -n +"$((KEEP + 1))" | while read -r d; do
    name="$(basename "$d")"
    [[ " ${keep[*]} " == *" $name "* ]] && continue
    log "removing old release $name"
    rm -rf -- "$d"
  done
}

if [ "$rollback" = 1 ]; then
  [ -f "$ROOT/.previous" ] || die "no previous release recorded"
  tag="$(cat "$ROOT/.previous")"
  valid_tag "$tag" && [ -f "$ROOT/$tag/.complete" ] || die "previous release '$tag' isn't installed"
  force=1
elif [ -z "$tag" ]; then
  # the timer (no arguments) leaves a held release alone; --resume and --force drop the hold
  if [ -f "$ROOT/.hold" ] && [ "$explicit" = 0 ]; then
    exit 0
  fi
  rm -f "$ROOT/.hold"
  tag="$(api -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases/latest" | jq -r .tag_name)"
fi
valid_tag "$tag" || die "not a release tag: '$tag'"

if [ "$tag" = "$current" ] && [ "$force" = 0 ]; then
  exit 0
fi
# A release that failed to start isn't tried again by the timer (it would take the site down
# every few minutes); tag a fixed one, or install it by hand with --tag.
if [ "$tag" = "$(cat "$ROOT/.failed" 2>/dev/null)" ] && [ "$explicit" = 0 ]; then
  exit 0
fi

fetch "$tag"
mapfile -t units < <(running_units)
log "installing $tag (was ${current:-none})"
switch_to "$tag"
if restart_and_check "${units[@]}"; then
  [ -n "$current" ] && [ "$current" != "$tag" ] && echo "$current" > "$ROOT/.previous"
  [ "$tag" != "$(cat "$ROOT/.failed" 2>/dev/null)" ] || rm -f "$ROOT/.failed"
  if [ "$rollback" = 1 ] || [ "$explicit_tag" = 1 ]; then
    echo "$tag" > "$ROOT/.hold"
    log "holding $tag: the timer won't install newer releases until vox-update.sh --resume"
  fi
  log "$tag is running"
  purge_cache
  prune "$tag"
else
  if [ -n "$current" ] && [ -f "$ROOT/$current/.complete" ]; then
    log "$tag failed; going back to $current"
    switch_to "$current"
    restart_and_check "${units[@]}" || log "$current didn't come back cleanly either; look at journalctl"
  fi
  echo "$tag" > "$ROOT/.failed"
  die "$tag failed to start; the timer won't try it again (vox-update.sh --tag $tag to retry)"
fi
