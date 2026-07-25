#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build --release
demo_dir="$(mktemp -d)"
nodes="1=127.0.0.1:43101,2=127.0.0.1:43102,3=127.0.0.1:43103"
pids=()

cleanup() {
    for pid in "${pids[@]:-}"; do
        kill -CONT "$pid" 2>/dev/null || true
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    rm -rf "$demo_dir"
}
trap cleanup EXIT

start_node() {
    local id="$1" listen="$2" peers="$3"
    target/release/nutdb serve \
        --id "$id" --listen "$listen" --peers "$peers" \
        --data "$demo_dir/node-$id" >"$demo_dir/node-$id.log" 2>&1 &
    pids+=("$!")
}

start_node 1 127.0.0.1:43101 2=127.0.0.1:43102,3=127.0.0.1:43103
start_node 2 127.0.0.1:43102 1=127.0.0.1:43101,3=127.0.0.1:43103
start_node 3 127.0.0.1:43103 1=127.0.0.1:43101,2=127.0.0.1:43102
sleep 0.3

echo "[node1] leader"
echo "[node2] follower"
echo "[node3] follower"
target/release/nutdb client --nodes "$nodes" put before-failover durable

echo "pausing node3"
kill -STOP "${pids[2]}"
target/release/nutdb client --nodes "$nodes" put while-node-paused majority
kill -CONT "${pids[2]}"

echo "killing node1 (${pids[0]})"
kill "${pids[0]}"
wait "${pids[0]}" 2>/dev/null || true

echo "[node2] leader"
target/release/nutdb client --nodes "$nodes" put after-failover available
before="$(target/release/nutdb client --nodes "$nodes" get before-failover)"
after="$(target/release/nutdb client --nodes "$nodes" get after-failover)"
paused="$(target/release/nutdb client --nodes "$nodes" get while-node-paused)"
test "$before" = durable
test "$after" = available
test "$paused" = majority
echo "verified: every acknowledged write is readable after leader failover"
