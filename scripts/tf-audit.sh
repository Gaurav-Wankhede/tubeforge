#!/bin/bash
# tf-audit.sh — one RPC session over the own-channel audit surface.
# Feeds line-delimited JSON-RPC into `tubeforge rpc`; results → /tmp/tf_audit.jsonl
set -u
TF="/Users/gauravwankhede/.cargo/bin/tubeforge"
ENVF="/Users/gauravwankhede/.tubeforge/.env"

ids=(n1UHtPoFRTc RSIiQJGvChM KEg24lxEHaQ vN_7i0Vxt4s 2Rzm1JUJxIo xRdFf8wBz1Y oMri2gCCWqE 7vFwPs5iofI 5i3TF7OrlXE 2aUbT9vjBYs EC667_j0c6U QIUBwKdQGvc 2zbbuypzmro I_jmzx3SK0Q)

{
  n=0
  for id in "${ids[@]}"; do
    printf '{"id":"s%d","method":"scores.detail","params":{"id":"%s"}}\n' "$n" "$id"
    n=$((n+1))
  done
  printf '{"id":"al","method":"alerts.list","params":{}}\n'
  printf '{"id":"hg","method":"health.get","params":{}}\n'
  printf '{"id":"ao","method":"analysis.overview","params":{}}\n'
} | timeout 1500 "$TF" --config "$ENVF" rpc > /tmp/tf_audit.jsonl 2>/tmp/tf_audit.err

echo "exit=$? results=$(grep -c '"type":"result"' /tmp/tf_audit.jsonl)"
