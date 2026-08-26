#!/usr/bin/env bash
# greedy-on-login.sh — TubeForge niche-filtered research at Mac login.
# Invoked by the LaunchAgent com.tubeforge.greedy-research (RunAtLoad).
# One batch per login; tubeforge's built-in 24h topic cooldown dedupes,
# so repeated same-day logins mostly no-op cheaply.
#
# Scope guard: ~/.tubeforge/.env TUBEFORGE_NICHE_TERMS filters candidates
# to the channel's niche BEFORE any research spend.

set -u

TF="/Users/gauravwankhede/.cargo/bin/tubeforge"
ENV_FILE="/Users/gauravwankhede/.tubeforge/.env"
LOG="/Users/gauravwankhede/.tubeforge/greedy-login.log"
LOCK="/Users/gauravwankhede/.tubeforge/.greedy-login.lock.d"

mkdir -p "$(dirname "$LOG")"

# Portable lock (no flock on macOS): atomic mkdir; steal if stale (>30 min).
if ! mkdir "$LOCK" 2>/dev/null; then
  now=$(date +%s)
  stamp=$(cat "$LOCK/stamp" 2>/dev/null || echo 0)
  if [ $((now - stamp)) -gt 1800 ]; then
    rm -rf "$LOCK"; mkdir "$LOCK" 2>/dev/null || exit 0
  else
    exit 0   # another run in flight; do not stack
  fi
fi
date +%s >"$LOCK/stamp"
trap 'rm -rf "$LOCK"' EXIT

log() { printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >>"$LOG"; }

log "--- login research start ---"

# Cap log growth: keep the last ~1000 lines.
if [ "$(wc -l <"$LOG" 2>/dev/null || echo 0)" -gt 2000 ]; then
  tail -n 1000 "$LOG" >"$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi

OUT="$("$TF" --config "$ENV_FILE" greedy run --max 5 --json 2>>"$LOG")"
RC=$?
log "greedy run exit=$RC payload=${OUT:0:400}"

HEALTH="$("$TF" --config "$ENV_FILE" health --json 2>>"$LOG")"
log "health: ${HEALTH:0:300}"

# Rotate a backup snapshot weekly-ish: keep N=10 (env), prune automatic.
if [ "$(date '+%u')" = "1" ]; then
  "$TF" --config "$ENV_FILE" backup --json >/dev/null 2>>"$LOG" \
    && log "weekly backup ok"
fi

log "--- login research done ---"
