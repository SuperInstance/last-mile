#!/usr/bin/env bash
# No floats in lm-core: no f32/f64 types, literals or casts anywhere under
# crates/lm-core/src. The crate lints catch arithmetic on floats, this catches
# the words themselves (including in comments and generated files).
set -euo pipefail
cd "$(dirname "$0")/.."

if grep -rnE '\bf(32|64)\b' crates/lm-core/src; then
    echo "check-no-float: float types found in crates/lm-core/src (matches above)" >&2
    exit 1
fi

echo "check-no-float: ok"
