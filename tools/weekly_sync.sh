#!/bin/bash
# Weekly incremental sync of every supported venue for the previous and current year.
#
# Run by launchd (com.akitenkrad.conf-scraper-sync, Wednesdays 03:00). `sync --incremental`
# re-fetches the listing of already-completed years for the current and previous year and
# fetches only papers it has not seen, so editions that were partially published at the
# last sync (e.g. NeurIPS 2025 with only its Creative AI track) are filled in over time.
#
#   bash tools/weekly_sync.sh            # same as the scheduled run
#
# Logs go to stdout/stderr; launchd redirects them to ~/Library/Logs/conf-scraper-sync.{log,err}.

set -u

BIN="${CONF_SCRAPER_BIN:-$HOME/.cargo/bin/conf-scraper}"
LOCK_DIR="$HOME/.cache/conf-scraper/.weekly-sync.lock"
# CiNii venues need CINII_APPID; without it the whole sync aborts at scraper construction.
CINII_VENUES="tjsai ipsj-jnl ieice-d jasag-sng"

log() { echo "$(date '+%F %T') $*"; }

if [ ! -x "$BIN" ]; then
  log "ERROR: $BIN not found or not executable"
  exit 1
fi

# --- single instance -----------------------------------------------------------------
mkdir -p "$(dirname "$LOCK_DIR")"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  other=$(cat "$LOCK_DIR/pid" 2>/dev/null || true)
  if [ -n "$other" ] && kill -0 "$other" 2>/dev/null; then
    log "SKIP: another weekly sync is running (pid $other)"
    exit 0
  fi
  log "WARN: removing stale lock (pid ${other:-unknown})"
  rm -rf "$LOCK_DIR"
  mkdir "$LOCK_DIR" || { log "ERROR: cannot create lock $LOCK_DIR"; exit 1; }
fi
echo $$ > "$LOCK_DIR/pid"
trap 'rm -rf "$LOCK_DIR"' EXIT

# --- CINII_APPID (launchd does not read ~/.zshrc) ---------------------------------------
if [ -z "${CINII_APPID:-}" ] && [ -f "$HOME/.zshrc" ]; then
  CINII_APPID=$(sed -n 's/^[[:space:]]*export[[:space:]]\{1,\}CINII_APPID=//p' "$HOME/.zshrc" | tail -1 | sed "s/^[\"']//; s/[\"']$//")
  export CINII_APPID
fi

# --- targets -------------------------------------------------------------------------
venues=$("$BIN" query catalog | /usr/bin/python3 -c 'import json,sys; print(",".join(v["id"] for v in json.load(sys.stdin)["venues"]))') || {
  log "ERROR: failed to read the venue catalog"
  exit 1
}
if [ -z "${CINII_APPID:-}" ]; then
  log "WARN: CINII_APPID is not set; skipping CiNii venues ($CINII_VENUES)"
  for v in $CINII_VENUES; do
    venues=$(echo ",$venues," | sed "s/,$v,/,/; s/^,//; s/,$//")
  done
fi

this_year=$(date +%Y)
years="$((this_year - 1))-$this_year"

log "START sync --year $years --incremental ($(echo "$venues" | tr ',' '\n' | wc -l | tr -d ' ') venues)"
"$BIN" sync --conference "$venues" --year "$years" --incremental
rc=$?
log "END exit=$rc"
exit $rc
