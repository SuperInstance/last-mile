#!/usr/bin/env bash
# No floats in lm-core, and none in lm-archive: no f32/f64 types, literals or
# casts anywhere under crates/lm-core/src or crates/lm-archive/src. The crate
# lints catch arithmetic on floats, this catches the words themselves
# (including in comments and generated files).
set -euo pipefail
cd "$(dirname "$0")/.."

for crate in crates/lm-core/src crates/lm-archive/src; do
    if grep -rnE '\bf(32|64)\b' "$crate"; then
        echo "check-no-float: float types found in $crate (matches above)" >&2
        exit 1
    fi
done

echo "check-no-float: ok"
