#!/usr/bin/env bash
# One release gate for behavior, architecture and repository claims.
#
# The battery runs the behavior suite, the architecture lint, and the living
# document/text/font gates.
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

run "cargo test" cargo test --locked --release --features gui
run "test policy self-test" python3 scripts/check_test_policy.py --self-test
run "test policy gate" python3 scripts/check_test_policy.py
run "cargo clippy" cargo clippy --locked --release --all-targets --features gui -- -D warnings
run "architecture self-test" python3 scripts/check_architecture.py --self-test
run "architecture gate" python3 scripts/check_architecture.py
run "text resources" python3 scripts/check_text.py
run "GUI font subsets" python3 scripts/subset_gui_fonts.py --check
run "doc drift" python3 scripts/check_doc_drift.py
run "release claims" python3 scripts/check_docs.py

echo "release battery green; transcript: $OUT" | tee -a "$OUT"
