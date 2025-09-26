#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BUILD_DIR=${BUILD_DIR:-"$ROOT_DIR/build"}
CONFIG=${CONFIG:-Release}
QT_CMAKE_BIN=${QT_CMAKE:-"$HOME/Qt/6.8.3/macos/bin/qt-cmake"}
CMAKE_ARGS=${CMAKE_ARGS:-""}

if [[ ! -x "$QT_CMAKE_BIN" ]]; then
    echo "error: qt-cmake not found at '$QT_CMAKE_BIN'. Set QT_CMAKE or install Qt 6.8.3." >&2
    exit 1
fi

mkdir -p "$BUILD_DIR"

"$QT_CMAKE_BIN" -S "$ROOT_DIR" -B "$BUILD_DIR" \
    -DCMAKE_BUILD_TYPE="$CONFIG" \
    -DQGC_ENABLE_GST_VIDEOSTREAMING=OFF \
    ${CMAKE_ARGS}

if command -v sysctl >/dev/null 2>&1; then
    BUILD_JOBS=${BUILD_JOBS:-$(sysctl -n hw.logicalcpu)}
elif command -v nproc >/dev/null 2>&1; then
    BUILD_JOBS=${BUILD_JOBS:-$(nproc)}
else
    BUILD_JOBS=${BUILD_JOBS:-4}
fi

cmake --build "$BUILD_DIR" --config "$CONFIG" -- -j"$BUILD_JOBS"

APP_BUNDLE="$BUILD_DIR/$CONFIG/QGroundControl.app"
APP_EXEC="$APP_BUNDLE/Contents/MacOS/QGroundControl"

if [[ ! -x "$APP_EXEC" ]]; then
    echo "error: expected executable '$APP_EXEC' not found." >&2
    exit 1
fi

exec "$APP_EXEC" "$@"
