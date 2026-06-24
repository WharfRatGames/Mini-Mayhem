#!/usr/bin/env bash
# audit_all_modes.sh — retroactively check every commit for all-modes coverage.
# Outputs a report of commits that touched gameplay files but missed some paths.
#
# Usage:
#   scripts/audit_all_modes.sh              # all commits
#   scripts/audit_all_modes.sh HEAD~20      # only last 20 commits
#   scripts/audit_all_modes.sh <sha>        # from that sha to HEAD

set -euo pipefail
REPO=$(git rev-parse --show-toplevel)
LOOP="src/game/loop_runner.rs"
MAIN="src/main.rs"
GAMEPLAY_GLOBS="src/game/ src/main.rs src/renderer/"

RED='\033[0;31m'; YEL='\033[1;33m'; GRN='\033[0;32m'; BLD='\033[1m'; DIM='\033[2m'; RST='\033[0m'

region_lines() {
    local file=$1 start_pat=$2 end_pat=$3
    local start end
    start=$(grep -n "$start_pat" "$REPO/$file" 2>/dev/null | head -1 | cut -d: -f1)
    end=$(grep -n "$end_pat" "$REPO/$file" 2>/dev/null | awk -F: -v s="${start:-0}" '$1 > s {print $1; exit}')
    echo "${start:-0} ${end:-0}"
}

diff_touches_region() {
    local difffile=$1 file_pat=$2 start=$3 end=$4
    [[ "$start" == "0" ]] && { echo "clean"; return; }
    awk -v fp="$file_pat" -v rs="$start" -v re="$end" '
        /^diff --git/ { in_file = ($0 ~ fp) }
        !in_file { next }
        /^@@ / {
            sub(/.*\+/, "", $0); sub(/[^0-9].*/, "", $0); cur = $0+0
        }
        /^[+\-]/ && !/^[+\-]{3}/ {
            if (cur >= rs && cur <= re) { found=1; exit }
            if ($1 ~ /^\+/) cur++
        }
        /^ / { cur++ }
        END { print (found ? "touched" : "clean") }
    ' "$difffile"
}

# Get current region line numbers (best approximation — they shift over history
# but we care about recent commits most)
read -r UC_S UC_E <<< "$(region_lines "$LOOP" "^fn update_camera"      "^pub fn tick\b")"
read -r TK_S TK_E <<< "$(region_lines "$LOOP" "^pub fn tick\b"         "^pub fn server_tick\b")"
read -r ST_S ST_E <<< "$(region_lines "$LOOP" "^pub fn server_tick\b"  "^pub fn ")"
read -r LC_S LC_E <<< "$(region_lines "$MAIN"  "let running = if net_conn" "if lstate.paused")"
read -r TR_S TR_E <<< "$(region_lines "$MAIN"  "OPPONENT'S MOVE"        "crate::audio::set_muted(true);")"
FF_LINE=$(grep -n "crate::audio::set_muted(true);" "$REPO/$MAIN" | awk -F: 'NR==2{print $1}')
FF_S=${FF_LINE:-0}; FF_E=$((FF_S + 80))

SINCE="${1:-$(git rev-list --max-parents=0 HEAD)}"
COMMITS=$(git log --oneline "${SINCE}..HEAD" 2>/dev/null || git log --oneline)

TOTAL=0; FLAGGED=0; SKIPPED=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

REPORT=""

while IFS= read -r line; do
    SHA=$(echo "$line" | cut -d' ' -f1)
    MSG=$(echo "$line" | cut -d' ' -f2-)
    TOTAL=$((TOTAL+1))

    # Get diff for this commit against its parent
    git diff "${SHA}^!" -- $GAMEPLAY_GLOBS > "$TMPFILE" 2>/dev/null || true

    if [[ ! -s "$TMPFILE" ]]; then
        SKIPPED=$((SKIPPED+1))
        continue
    fi

    # Check each path
    r1=$(diff_touches_region "$TMPFILE" "loop_runner" "$UC_S" "$UC_E")
    r2=$(diff_touches_region "$TMPFILE" "loop_runner" "$TK_S" "$TK_E")
    r3=$(diff_touches_region "$TMPFILE" "loop_runner" "$ST_S" "$ST_E")
    r4=$(diff_touches_region "$TMPFILE" "main.rs"     "$LC_S" "$LC_E")
    r5=$(diff_touches_region "$TMPFILE" "main.rs"     "$TR_S" "$TR_E")
    r6=$(diff_touches_region "$TMPFILE" "main.rs"     "$FF_S" "$FF_E")

    touched=0; missed=0; missed_names=""
    for pair in "update_camera:$r1" "tick():$r2" "server_tick():$r3" "live_client:$r4" "tat_replay:$r5" "tat_ff:$r6"; do
        name=${pair%%:*}; val=${pair##*:}
        if [[ "$val" == "touched" ]]; then touched=$((touched+1))
        else missed=$((missed+1)); missed_names="$missed_names $name"
        fi
    done

    # Only flag if at least one path was touched (pure-doc/non-gameplay changes won't touch any)
    if [[ $touched -eq 0 ]]; then
        SKIPPED=$((SKIPPED+1))
        continue
    fi

    if [[ $missed -gt 0 ]]; then
        FLAGGED=$((FLAGGED+1))
        REPORT="${REPORT}\n${YEL}${SHA}${RST} ${MSG}"
        REPORT="${REPORT}\n         touched=$touched  missed=$missed:${RED}${missed_names}${RST}\n"
    fi

done <<< "$COMMITS"

echo ""
echo -e "${BLD}=== All-modes retroactive audit ===${RST}"
echo -e "Commits scanned: $TOTAL  |  No gameplay changes: $SKIPPED  |  Flagged: ${FLAGGED}"
echo ""

if [[ -n "$REPORT" ]]; then
    echo -e "$REPORT"
    echo -e "${YEL}${BLD}${FLAGGED} commit(s) had gameplay changes that missed at least one path.${RST}"
    echo -e "${DIM}Note: line numbers are based on current HEAD — some flags on old commits may be false positives due to line drift.${RST}"
else
    echo -e "${GRN}${BLD}No issues found.${RST}"
fi
