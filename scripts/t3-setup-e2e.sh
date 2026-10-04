#!/bin/sh
# Real native onboarding, attached discovery, pinned download and restart.
# BUKNO_EVIDENCE_DIR=/mnt/toshiba/dev/artifacts/bukno/... sh scripts/t3-setup-e2e.sh
set -eu
: "${BUKNO_EVIDENCE_DIR:?set BUKNO_EVIDENCE_DIR}"
export BUKNO_T3_SETUP_LIVE=1 BUKNO_T3_AUTO_SETUP=1
export CARGO_TARGET_DIR="${BUKNO_CARGO_TARGET_DIR:-$HOME/.cache/bukno/cargo-target}"
root="$(mktemp -d "${TMPDIR:-/tmp}/bukno-t3-setup-XXXXXX")"
export BUKNO_T3_SETUP_ROOT="$root"
evidence="$BUKNO_EVIDENCE_DIR"
mkdir -p "$evidence"
BUKNO_T3_SETUP_ROOT="$root/attached" BUKNO_T3_SETUP_MODE=attached BUKNO_T3_HOME="${T3CODE_HOME:-$HOME/.t3}" \
BUKNO_EVIDENCE_DIR="$evidence/attached" \
  cargo test --locked -p bukno-desktop --test t3_setup -- --nocapture --test-threads=1
# A nonexistent explicit CLI prevents detection of the daily-driver app. The
# downloader and server remain entirely in this test's isolated state folder.
BUKNO_T3_SETUP_ROOT="$root/download" BUKNO_T3_SETUP_MODE=download BUKNO_T3_HOME="$root/no-existing-t3" BUKNO_T3_CLI="$root/missing-t3" \
BUKNO_EVIDENCE_DIR="$evidence/download" \
  cargo test --locked -p bukno-desktop --test t3_setup -- --nocapture --test-threads=1
BUKNO_T3_SETUP_ROOT="$root/repair" BUKNO_T3_SETUP_MODE=repair BUKNO_T3_HOME="$root/no-existing-t3" BUKNO_T3_CLI="$root/missing-t3" \
BUKNO_EVIDENCE_DIR="$evidence/repair" \
  cargo test --locked -p bukno-desktop --test t3_setup -- --nocapture --test-threads=1
# A separate root keeps the downloaded server from the failure scenario.
BUKNO_T3_SETUP_ROOT="$root/invalid" BUKNO_T3_SETUP_MODE=invalid-runtime \
BUKNO_T3_HOME="$root/invalid-home" BUKNO_T3_CLI="$root/missing-t3" \
BUKNO_EVIDENCE_DIR="$evidence/invalid-runtime" \
  cargo test --locked -p bukno-desktop --test t3_setup -- --nocapture --test-threads=1
BUKNO_T3_SETUP_ROOT="$root/offline" BUKNO_T3_SETUP_MODE=offline BUKNO_T3_HOME="$root/offline-home" BUKNO_T3_CLI="$root/missing-t3" HTTPS_PROXY=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 BUKNO_EVIDENCE_DIR="$evidence/offline"   cargo test --locked -p bukno-desktop --test t3_setup -- --nocapture --test-threads=1
# On failure, keep the isolated state for inspection. After success the test
# has stopped its disposable managed server and the rebuildable runtime can go.
rm -rf "$root"
printf 'All setup checks passed. Evidence: %s\n' "$evidence"
