#!/usr/bin/env bash
# Build sysone-batched against the llama.cpp build that is already on the Thor.
#
# Nothing is fetched: the sources of the fork are in ~/llama.cpp-upstream and
# the libraries are in ~/llama.cpp-upstream/build-up/bin. The program links
# against them, so the decision context that renders the questions is the very
# same code the fork's own server uses.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
LLAMA="${LLAMA_ROOT:-$HOME/llama.cpp-upstream}"
BUILD="${LLAMA_BUILD:-$LLAMA/build-up}"

if [ ! -d "$LLAMA" ]; then
    echo "no llama.cpp tree at $LLAMA" >&2
    exit 1
fi
if [ ! -f "$BUILD/bin/libllama.so" ]; then
    echo "no llama.cpp build at $BUILD" >&2
    exit 1
fi

CXX="${CXX:-c++}"

INCLUDES=(
    -I"$LLAMA/include"
    -I"$LLAMA/ggml/include"
    -I"$LLAMA/common"
    -I"$LLAMA/tools/server"
    -I"$LLAMA/tools/mtmd"
    -I"$LLAMA/vendor/cpp-httplib"
    -I"$LLAMA/vendor/nlohmann"
)

# the runtime library path of the build
RPATH="-Wl,-rpath,$BUILD/bin"

# the server, and the two probes that prove what it does and cannot do
for prog in sysone-batched probe-batching probe-concurrency; do
    "$CXX" -std=c++17 -O2 -pthread -fPIC \
        "${INCLUDES[@]}" \
        "$HERE/$prog.cpp" \
        -o "$HERE/$prog" \
        -L"$BUILD/bin" \
        $RPATH \
        -lllama-server-impl -lllama-common -lllama -lmtmd -lggml -lggml-base -lggml-cpu -lggml-cuda \
        -Wl,-rpath,"$BUILD/bin"
    echo "built $HERE/$prog"
done
