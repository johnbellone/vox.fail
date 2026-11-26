#!/usr/bin/env bash
# Run the Void, the yard and the islands locally (guests allowed, so portals work) and join the
# Void with the desktop client.
# Usage: tools/dev/play-local.sh [client args...]   e.g. --name Sam --model models/dog.glb
set -euo pipefail
cd "$(dirname "$0")/../.."

for port in 7780 7781 7782; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    echo "port $port is already in use (a leftover server? try: pkill vox-server)" >&2
    exit 1
  fi
done

cargo build --release -p vox-server -p vox-client

pids=()
start() { # name port seed
  ./target/release/vox-server --listen "127.0.0.1:$2" --name "$1" --db "$1.db" --seed "data/levels/$3.json" &
  pids+=($!)
}
start void 7780 void
start yard 7781 yard
start islands 7782 islands
trap 'kill -INT "${pids[@]}" 2>/dev/null; wait 2>/dev/null || true' EXIT
sleep 1

# your --name wins: the client takes the first one it sees
name=()
[[ " $* " == *" --name "* ]] || name=(--name "${USER:-Guest}")
./target/release/vox-client --world ws://127.0.0.1:7780/ws "$@" "${name[@]}"
