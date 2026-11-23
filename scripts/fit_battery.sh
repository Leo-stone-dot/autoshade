#!/usr/bin/env bash
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

# The deterministic fit integration battery: degenerate-pair refusal and base
# preservation, the global fit workflow, and the zoned segmentation-failure
# fallback. These cases execute public entry points and real rendering, so they
# are explicit rather than part of the default fast suite.
cargo test --locked --release --features gui --test fit_behavior -- --ignored
cargo test --locked --release --features gui --test fit_workflow -- --ignored
