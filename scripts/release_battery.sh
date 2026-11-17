#!/usr/bin/env bash
# One fast release gate: lint and the document/text/font gates.
#
# No behavior cases are currently checked in; the battery proves the code still
# passes the architecture lint and that the living documents remain accurate.
#
#   scripts/release_battery.sh
#   scripts/release_battery.sh --out target/battery.txt

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT="$REPO/target/battery.txt"

while [ $# -gt 0 ]; do
    case "$1" in
        --out) OUT=$2; shift 2 ;;
        -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "release_battery: unknown argument $1" >&2; exit 2 ;;
    esac
done

mkdir -p "$(dirname "$OUT")"
: > "$OUT"
cd "$REPO"

run() {
    name=$1
    shift
    echo "=== $name ===" | tee -a "$OUT"
    "$@" 2>&1 | tee -a "$OUT"
    echo | tee -a "$OUT"
}

run "cargo clippy" cargo clippy --locked --release --all-targets --features gui -- -D warnings
run "text resources" python3 scripts/check_text.py
run "GUI font subsets" python3 scripts/subset_gui_fonts.py --check
run "doc drift" python3 scripts/check_doc_drift.py
run "release claims" python3 scripts/check_docs.py

echo "release battery green; transcript: $OUT" | tee -a "$OUT"
