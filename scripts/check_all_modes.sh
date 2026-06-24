#!/usr/bin/env bash
# check_all_modes.sh — verify that gameplay changes touch all 5 code paths.
#
# Usage:
#   scripts/check_all_modes.sh          # check staged diff (pre-commit)
#   scripts/check_all_modes.sh HEAD~1   # check last commit
#   scripts/check_all_modes.sh show     # show current state of all 5 regions
#
# The 5 paths:
#   1. loop_runner.rs :: update_camera()        — hotseat/vs-CPU camera
#   2. loop_runner.rs :: tick()                 — hotseat/vs-CPU sim
#   3. loop_runner.rs :: server_tick()          — live server sim
#   4. main.rs        :: live client block      — live camera + input
#   5. main.rs        :: TAT visual replay      — opponent's move
#   6. main.rs        :: TAT fast-forward       — own move (no render)

set -euo pipefail
REPO=$(git rev-parse --show-toplevel)
LOOP="$REPO/src/game/loop_runner.rs"
MAIN="$REPO/src/main.rs"

RED='\033[0;31m'; YEL='\033[1;33m'; GRN='\033[0;32m'; BLD='\033[1m'; RST='\033[0m'

# ── Extract a region from a file between two line-number anchors ──────────────
region_lines() {
    local file=$1 start_pat=$2 end_pat=$3
    local start end
    start=$(grep -n "$start_pat" "$file" | head -1 | cut -d: -f1)
    end=$(grep -n "$end_pat" "$file" | awk -F: -v s="$start" '$1 > s {print $1; exit}')
    if [[ -z "$start" ]]; then echo "0 0"; return; fi
    if [[ -z "$end" ]];   then end=$(wc -l < "$file"); fi
    echo "$start $end"
}

# ── Detect which lines of a file were touched in a diff ──────────────────────
# Prints "touched" or "clean"
diff_touches_region() {
    local diff=$1 file_pat=$2 start=$3 end=$4
    # Parse unified diff for +/- lines in the right file/range
    awk -v fp="$file_pat" -v rs="$start" -v re="$end" '
        /^diff --git/ { in_file = ($0 ~ fp) }
        !in_file { next }
        /^@@ / {
            # parse @@ -old,n +new,m @@ — extract the +NNN part
            sub(/.*\+/, "", $0); sub(/[^0-9].*/, "", $0); cur = $0+0
        }
        /^[+\-]/ && !/^[+\-]{3}/ {
            if (cur >= rs && cur <= re) { found=1; exit }
            if ($1 ~ /^\+/) cur++
        }
        /^ / { cur++ }
        END { print (found ? "touched" : "clean") }
    ' "$diff"
}

SHOW_MODE=0
REF=""
if [[ "${1:-}" == "show" ]]; then SHOW_MODE=1
elif [[ -n "${1:-}" ]];      then REF="$1"
fi

# ── show mode: print each region ─────────────────────────────────────────────
if [[ $SHOW_MODE -eq 1 ]]; then
    show_region() {
        local label=$1 file=$2 start_pat=$3 end_pat=$4 maxlines=${5:-60}
        read -r s e <<< "$(region_lines "$file" "$start_pat" "$end_pat")"
        echo -e "\n${BLD}=== $label (lines $s–$e) ===${RST}"
        sed -n "${s},$((s + maxlines))p" "$file"
        [[ $((e - s)) -gt $maxlines ]] && echo "  ... (truncated)"
    }
    show_region "1. update_camera (loop_runner.rs)" "$LOOP" "^fn update_camera" "^pub fn tick\b" 80
    show_region "2. tick() (loop_runner.rs)"         "$LOOP" "^pub fn tick\b"    "^pub fn server_tick\b" 80
    show_region "3. server_tick() (loop_runner.rs)"  "$LOOP" "^pub fn server_tick\b" "^pub fn " 80
    show_region "4. Live client block (main.rs)"     "$MAIN" "let running = if net_conn" "if lstate.paused" 80
    show_region "5. TAT visual replay (main.rs)"     "$MAIN" "OPPONENT'S MOVE" "crate::audio::set_muted(true);" 80
    show_region "6. TAT fast-forward (main.rs)"      "$MAIN" "crate::audio::set_muted(true);" "^}" 80
    exit 0
fi

# ── diff mode ─────────────────────────────────────────────────────────────────
TMPDIR_CUSTOM=$(mktemp -d)
trap 'rm -rf "$TMPDIR_CUSTOM"' EXIT
DIFFFILE="$TMPDIR_CUSTOM/staged.diff"

if [[ -n "$REF" ]]; then
    git diff "$REF" -- src/game/loop_runner.rs src/main.rs src/game/ src/renderer/ > "$DIFFFILE"
else
    git diff --cached -- src/game/loop_runner.rs src/main.rs src/game/ src/renderer/ > "$DIFFFILE"
fi

if [[ ! -s "$DIFFFILE" ]]; then
    echo -e "${GRN}No gameplay files changed — all-modes check skipped.${RST}"
    exit 0
fi

# Determine region line ranges
read -r UC_S UC_E   <<< "$(region_lines "$LOOP" "^fn update_camera"      "^pub fn tick\b")"
read -r TK_S TK_E   <<< "$(region_lines "$LOOP" "^pub fn tick\b"         "^pub fn server_tick\b")"
read -r ST_S ST_E   <<< "$(region_lines "$LOOP" "^pub fn server_tick\b"  "^pub fn ")"
read -r LC_S LC_E   <<< "$(region_lines "$MAIN"  "let running = if net_conn" "if lstate.paused")"
read -r TR_S TR_E   <<< "$(region_lines "$MAIN"  "OPPONENT'S MOVE"        "crate::audio::set_muted(true);")"
# TAT fast-forward: second occurrence of set_muted(true)
FF_LINE=$(grep -n "crate::audio::set_muted(true);" "$MAIN" | awk -F: 'NR==2{print $1}')
FF_S=${FF_LINE:-0}; FF_E=$(( FF_S + 60 ))

check() {
    local label=$1 file_pat=$2 s=$3 e=$4
    local result
    result=$(diff_touches_region "$DIFFFILE" "$file_pat" "$s" "$e")
    if [[ "$result" == "touched" ]]; then
        echo -e "  ${GRN}✓${RST}  $label"
        return 0
    else
        echo -e "  ${YEL}–${RST}  $label"
        return 1
    fi
}

echo -e "\n${BLD}All-modes check — gameplay diff detected${RST}"
echo "Checking which of the 5 paths were touched:"
echo ""

MISSED=0
check "1. update_camera()     loop_runner.rs:$UC_S" "loop_runner" "$UC_S" "$UC_E" || MISSED=$((MISSED+1))
check "2. tick()              loop_runner.rs:$TK_S" "loop_runner" "$TK_S" "$TK_E" || MISSED=$((MISSED+1))
check "3. server_tick()       loop_runner.rs:$ST_S" "loop_runner" "$ST_S" "$ST_E" || MISSED=$((MISSED+1))
check "4. Live client block   main.rs:$LC_S"        "main.rs"     "$LC_S" "$LC_E" || MISSED=$((MISSED+1))
check "5. TAT visual replay   main.rs:$TR_S"        "main.rs"     "$TR_S" "$TR_E" || MISSED=$((MISSED+1))
check "6. TAT fast-forward    main.rs:$FF_S"        "main.rs"     "$FF_S" "$FF_E" || MISSED=$((MISSED+1))

echo ""
if [[ $MISSED -gt 0 ]]; then
    echo -e "${YEL}${BLD}WARNING: $MISSED path(s) not touched.${RST}"
    echo -e "${YEL}If this change applies to those paths, update them before committing.${RST}"
    echo -e "${YEL}To skip this check: SKIP_MODES_CHECK=1 git commit ...${RST}"
    echo ""
    if [[ "${SKIP_MODES_CHECK:-}" == "1" ]]; then
        echo -e "${YEL}SKIP_MODES_CHECK=1 set — proceeding anyway.${RST}"
        exit 0
    fi
    # In pre-commit hook context, exit 1 blocks the commit.
    # Running standalone: just warn, don't block.
    [[ "${ALL_MODES_HOOK:-}" == "1" ]] && exit 1
    exit 0
else
    echo -e "${GRN}${BLD}All touched paths accounted for.${RST}"
fi
