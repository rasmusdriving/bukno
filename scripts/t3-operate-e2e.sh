#!/bin/sh
# Run the Stage 2 T3 end-to-end check against the T3 server on this machine,
# and record a demo video of it.
#
#   BUKNO_EVIDENCE_DIR=… BUKNO_T3_ADDRESS=http://127.0.0.1:3773 \
#     sh scripts/t3-operate-e2e.sh
#
# Creates a disposable Git project with an uncommitted change on the internal
# drive, adds it to T3 with T3's own `t3 project add`, mints one pairing link
# into a private temporary file (never printed), runs
# apps/desktop/tests/t3_operate.rs, then encodes the recorded frames into
# demo.mp4 in the evidence folder. Uses a little Codex and Claude usage.
# See e2e/scenarios/t3-operate.md.
set -eu
export BUKNO_T3_AUTO_SETUP=0

: "${BUKNO_EVIDENCE_DIR:?set BUKNO_EVIDENCE_DIR}"
: "${BUKNO_T3_ADDRESS:?set BUKNO_T3_ADDRESS}"
BUKNO_T3_CLI="${BUKNO_T3_CLI:-/opt/T3 Code (Nightly)/t3code}"
BUKNO_T3_CLI_ENTRY="${BUKNO_T3_CLI_ENTRY:-/opt/T3 Code (Nightly)/resources/app.asar/apps/server/dist/bin.mjs}"
export BUKNO_T3_CLI BUKNO_T3_CLI_ENTRY
t3() { ELECTRON_RUN_AS_NODE=1 "$BUKNO_T3_CLI" "$BUKNO_T3_CLI_ENTRY" "$@"; }

stamp="$(date +%Y%m%d-%H%M%S)"
work="${BUKNO_E2E_WORK:-$HOME/.cache/bukno-e2e}/t3-operate-$stamp"
project="$work/project"
mkdir -p "$project"
git -C "$project" init -q -b main
printf '# Disposable project for the Bukno Stage 2 check\n' > "$project/README.md"
printf 'Committed line.\n' > "$project/notes.md"
git -C "$project" add -A
git -C "$project" -c user.name=bukno-e2e -c user.email=e2e@localhost commit -q -m "Start"
# An uncommitted change the chats are not asked to touch.
printf 'Uncommitted line that must survive.\n' >> "$project/notes.md"
title="Bukno Stage 2 check $stamp"
t3 project add --title "$title" "$project" >/dev/null

umask 077
links="$(mktemp)"
trap 'rm -f "$links"' EXIT INT TERM
t3 pair --ttl 30m --label "Bukno Stage 2 check" 2>/dev/null \
    | grep -o 'http[s]*://[^ ]*/pair#token=[^ ]*' | head -n 1 > "$links"
[ -s "$links" ] || { echo "could not mint a pairing link" >&2; exit 1; }
umask 022

mkdir -p "$BUKNO_EVIDENCE_DIR"
video="$work/video"
status=0
BUKNO_T3_LIVE=1 BUKNO_T3_LINKS_FILE="$links" BUKNO_T3_PROJECT_ROOT="$project" BUKNO_T3_PROJECT_TITLE="$title" \
BUKNO_VIDEO_DIR="$video" BUKNO_REDUCED_MOTION="${BUKNO_REDUCED_MOTION:-0}" \
    cargo test --locked -p bukno-desktop --test t3_operate -- --nocapture --test-threads=1 || status=$?

# Frames stay on the internal drive; only the encoded video goes to evidence.
if [ -d "$video/frames" ] && [ -n "$(ls -A "$video/frames")" ]; then
    gst-launch-1.0 -q -e \
        multifilesrc location="$video/frames/%06d.png" index=0 caps="image/png,framerate=10/1" \
        ! pngdec ! videoconvert ! videobox bottom=-72 fill=black ! overlay.video_sink \
        filesrc location="$video/captions.srt" ! subparse ! overlay.text_sink \
        textoverlay name=overlay font-desc="Sans 9" valignment=bottom halignment=center ypad=22 \
            wrap-mode=word color=0xffe8e8e8 \
        ! videoconvert ! video/x-raw,format=I420 \
        ! x264enc speed-preset=medium pass=quant quantizer=20 key-int-max=50 \
        ! mp4mux ! filesink location="$BUKNO_EVIDENCE_DIR/demo.mp4"
    cp "$video/captions.srt" "$BUKNO_EVIDENCE_DIR/captions.srt"
    echo "video: $BUKNO_EVIDENCE_DIR/demo.mp4 ($(ls "$video/frames" | wc -l) frames)"
fi
echo "project left in T3 for inspection: $title ($project)"
exit "$status"
