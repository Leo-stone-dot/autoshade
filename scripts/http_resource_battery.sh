#!/usr/bin/env bash
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

# Real 256 MiB requests and repeated failure recovery, never reduced test limits.
cargo test --locked --release --features gui \
    --test serve_behavior http_limits:: -- --ignored --test-threads=1
cargo test --locked --release --features gui --test serve_behavior \
    http_workflows::failed_heals_do_not_exhaust_the_capacity_for_a_later_valid_heal \
    -- --ignored --exact
