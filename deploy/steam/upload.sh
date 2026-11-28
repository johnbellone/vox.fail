#!/usr/bin/env bash
# Upload a build of vox-client to Steam (SteamPipe) and set it live on a test branch. Run by
# .github/workflows/steam.yml; see deploy/steam/README.md for the Steamworks side.
#
#   deploy/steam/upload.sh --app 1234560 --branch beta --desc "v0.2.0 (abc1234)" \
#     --root steam --depot 1234561=windows --depot 1234562=linux [--output <dir>] [--dry-run]
#
# --root is the folder holding every depot's folder; each --depot is a depot id and its folder
# under the root, which becomes that depot's whole contents. --output is where steamcmd writes
# its build logs (a temporary folder, removed afterwards, if not given). With --dry-run the build
# script is printed and steamcmd isn't run. Otherwise it runs
# `$STEAMCMD +login $STEAM_USERNAME +run_app_build ...`, which signs in from the saved login in
# steamcmd's config/config.vdf (no password: see the README), and fails unless steamcmd says the
# build finished: steamcmd can exit 0 after a failed build.
#
# The build is never set live on the default branch: that is a person's decision, made on the
# Steamworks Builds page after trying the build on the test branch.
set -euo pipefail

app="" branch="" desc="" root="" output="" dry=0
depots=()
while [ $# -gt 0 ]; do
  case "$1" in
    --app) app="${2:?}"; shift 2 ;;
    --branch) branch="${2:?}"; shift 2 ;;
    --desc) desc="${2:?}"; shift 2 ;;
    --root) root="${2:?}"; shift 2 ;;
    --depot) depots+=("${2:?}"); shift 2 ;;
    --output) output="${2:?}"; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) echo "upload.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

fail() { echo "upload.sh: $*" >&2; exit 1; }
# Everything goes into a VDF file between double quotes, so a path may not contain one (or a
# backslash, VDF's escape).
plain_path() { [[ "$1" != *'"'* && "$1" != *\\* ]]; }

[[ "$app" =~ ^[0-9]+$ ]] || fail "--app must be a Steam app id (digits), got '$app'"
[[ "$branch" =~ ^[A-Za-z0-9_-]+$ ]] || fail "--branch must be a branch name (letters, digits, - and _), got '$branch'"
[ "$branch" != default ] || fail "builds are set live on 'default' by hand in Steamworks, not from here"
[ -n "$desc" ] || fail "--desc is required"
[[ "$desc" =~ ^[A-Za-z0-9\ ._()+:/-]+$ ]] || fail "--desc has characters a build description can't hold: '$desc'"
[ -d "$root" ] || fail "--root must be a folder, got '$root'"
root=$(cd "$root" && pwd)
plain_path "$root" || fail "the root's path can't contain \" or \\"
[ ${#depots[@]} -gt 0 ] || fail "at least one --depot <id>=<folder> is required"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [ -z "$output" ]; then output="$work/output"; fi
mkdir -p "$output"
output=$(cd "$output" && pwd)
plain_path "$output" || fail "the output folder's path can't contain \" or \\"

entries=""
for d in "${depots[@]}"; do
  id="${d%%=*}"
  dir="${d#*=}"
  [[ "$id" =~ ^[0-9]+$ ]] || fail "depot id must be digits, got '$id'"
  [[ "$dir" =~ ^[A-Za-z0-9_-]+$ ]] || fail "depot $id: the folder must be one plain name under the root, got '$dir'"
  [ -d "$root/$dir" ] || fail "depot $id: no folder $root/$dir"
  [ -n "$(ls -A "$root/$dir")" ] || fail "depot $id: $root/$dir is empty"
  entries+="    \"$id\"
    {
      \"FileMapping\" { \"LocalPath\" \"$dir/*\" \"DepotPath\" \".\" \"recursive\" \"1\" }
    }
"
done

cat > "$work/app_build.vdf" <<EOF
"AppBuild"
{
  "AppID" "$app"
  "Desc" "$desc"
  "ContentRoot" "$root"
  "BuildOutput" "$output"
  "SetLive" "$branch"
  "Depots"
  {
$entries  }
}
EOF

if [ "$dry" = 1 ]; then
  cat "$work/app_build.vdf"
  exit 0
fi

: "${STEAMCMD:?path to steamcmd.sh}"
: "${STEAM_USERNAME:?the Steam user name of the build account}"
# no input: an expired login makes steamcmd ask for a password, and it should fail, not wait
"$STEAMCMD" +login "$STEAM_USERNAME" +run_app_build "$work/app_build.vdf" +quit </dev/null | tee "$work/steamcmd.log"
grep -q "Successfully finished AppID $app build" "$work/steamcmd.log" \
  || fail "steamcmd did not report a finished build (an expired saved login? see deploy/steam/README.md)"
