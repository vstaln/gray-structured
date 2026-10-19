#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/gray-structured.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/gray-structured" ;;
    *) rel="x86_64-apple-darwin/gray-structured" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/gray-structured" ;;
    *) rel="x86_64-unknown-linux-musl/gray-structured" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/gray-structured.exe" "$here/target/release/gray-structured" "$here/target/debug/gray-structured.exe" "$here/target/debug/gray-structured"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "gray-structured binary not found in $here" >&2
exit 1
