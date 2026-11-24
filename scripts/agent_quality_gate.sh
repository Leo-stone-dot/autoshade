#!/usr/bin/env bash
# The gate an agent runs before claiming a code change is complete.
#
# It is intentionally thin: the Rust toolchain and cargo-mutants own the
# verdict. The wrapper only supplies the agent's changed-file scope.
#
#   scripts/agent_quality_gate.sh
#   AUTOSHADE_MUTATION_JOBS=4 scripts/agent_quality_gate.sh
#   AUTOSHADE_MUTATION_FILES=src/fit_zoned/attach.rs scripts/agent_quality_gate.sh
#
# A test-only change must name the production behavior it protects. The
# mutation wrapper fails instead of silently skipping when that scope is absent.

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

cargo test --locked --release --features gui
cargo clippy --locked --release --all-targets --features gui -- -D warnings
python3 scripts/check_test_policy.py --self-test
python3 scripts/check_test_policy.py
python3 scripts/check_text.py
python3 scripts/check_doc_drift.py
python3 scripts/check_docs.py

if [[ "${AUTOSHADE_FIT_TESTS:-0}" == "1" ]]; then
    scripts/fit_battery.sh
fi

if [[ "${AUTOSHADE_HTTP_RESOURCE_TESTS:-0}" == "1" ]]; then
    scripts/http_resource_battery.sh
fi

python3 scripts/mutation_gate.py \
    --worktree \
    --jobs "${AUTOSHADE_MUTATION_JOBS:-2}" \
    --test-tool "${AUTOSHADE_MUTATION_TEST_TOOL:-cargo}"
