#!/bin/sh
# Run the Stage 1 read-only T3 end-to-end check against the T3 server on this
# machine. Mints two fresh pairing links with T3's own `t3 pair` into a private
# temporary file (never printed), then runs apps/desktop/tests/t3_live.rs.
#
#   BUKNO_EVIDENCE_DIR=… BUKNO_T3_ADDRESS=http://100.127.119.35:3773 \
#   BUKNO_T3_LONG_THREAD=… BUKNO_T3_PAGED_THREAD=… BUKNO_T3_LIVE_THREAD=… \
#     sh scripts/t3-live-e2e.sh
#
# See e2e/scenarios/t3-read-only.md.
set -eu

: "${BUKNO_EVIDENCE_DIR:?set BUKNO_EVIDENCE_DIR}"
: "${BUKNO_T3_ADDRESS:?set BUKNO_T3_ADDRESS}"
: "${BUKNO_T3_LONG_THREAD:?}" "${BUKNO_T3_PAGED_THREAD:?}" "${BUKNO_T3_LIVE_THREAD:?}"
BUKNO_T3_CLI="${BUKNO_T3_CLI:-/opt/T3 Code (Nightly)/t3code}"
BUKNO_T3_CLI_ENTRY="${BUKNO_T3_CLI_ENTRY:-/opt/T3 Code (Nightly)/resources/app.asar/apps/server/dist/bin.mjs}"
export BUKNO_T3_CLI BUKNO_T3_CLI_ENTRY

umask 077
links="$(mktemp)"
trap 'rm -f "$links"' EXIT INT TERM
for ttl in 30m 60m; do
    # Keep only the link line; the command also prints the bare token.
    ELECTRON_RUN_AS_NODE=1 "$BUKNO_T3_CLI" "$BUKNO_T3_CLI_ENTRY" pair --ttl "$ttl" --label "Bukno end-to-end check" 2>/dev/null \
        | grep -o 'http[s]*://[^ ]*/pair#token=[^ ]*' | head -n 1 >> "$links"
done
[ "$(wc -l < "$links")" -eq 2 ] || { echo "could not mint two pairing links" >&2; exit 1; }

mkdir -p "$BUKNO_EVIDENCE_DIR"
BUKNO_T3_LIVE=1 BUKNO_T3_LINKS_FILE="$links" \
    cargo test --locked -p bukno-desktop --test t3_live -- --nocapture --test-threads=1
