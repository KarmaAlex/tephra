#!/usr/bin/env bash
# Cross-compiles obsidilight.exe for x86_64 Windows. Runs INSIDE the obsidilight-build-windows
# container (packaging/windows/Containerfile); started by `make windows`.
#   in : the repo, mounted at $PWD
#   out: dist/obsidilight-<version>-windows-x86_64.zip  (obsidilight.exe + licenses)
# Native build on a Windows machine instead: packaging/windows/build.ps1
set -euo pipefail

TARGET=x86_64-pc-windows-gnu
export CARGO_TARGET_DIR=target/cross/windows
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
# Self-contained exe: no libgcc_s_seh-1.dll / libstdc++-6.dll / libwinpthread-1.dll next to it.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C target-feature=+crt-static -C link-args=-static"

# GPUI's Windows backend precompiles its HLSL shaders with fxc.exe only when the build *host* is
# Windows; patch in a copy of the crate whose build script uses vkd3d-compiler instead.
ver=$(awk '/^name = "gpui-pre-windows"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock)
[[ -n $ver ]] || { echo "gpui-pre-windows not found in Cargo.lock" >&2; exit 1; }
cargo fetch --locked --target "$TARGET" >/dev/null
orig=$(echo "$CARGO_HOME"/registry/src/*/gpui-pre-windows-"$ver")
patched=$CARGO_TARGET_DIR/patched/gpui-pre-windows-$ver
rm -rf "$patched" && mkdir -p "$(dirname "$patched")" && cp -r "$orig" "$patched"
cp packaging/windows/gpui-windows-build.rs "$patched/build.rs"

# The [patch] rewrites gpui-pre-windows's Cargo.lock entry; keep the committed lockfile intact.
cp Cargo.lock "$CARGO_TARGET_DIR/Cargo.lock.orig"
trap 'cp "$CARGO_TARGET_DIR/Cargo.lock.orig" Cargo.lock' EXIT

# (no --locked: the [patch] has to rewrite the lock entry; the fetch above was --locked)
cargo build --release --target "$TARGET" -p obsidilight \
    --config "patch.crates-io.gpui-pre-windows.path=\"$patched\""

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
Z=$CARGO_TARGET_DIR/zip/obsidilight-$VERSION-windows-x86_64
rm -rf "$Z" && mkdir -p "$Z" dist
cp "$CARGO_TARGET_DIR/$TARGET/release/obsidilight.exe" "$Z/"
cp LICENSE "$Z/LICENSE.txt"
cp crates/app/assets/fonts/LICENSE-IBMPlexSans.txt crates/app/assets/fonts/LICENSE-DejaVu.txt "$Z/"
cp packaging/windows/README.txt "$Z/README.txt"
sed -i 's/$/\r/' "$Z"/*.txt  # CRLF for Notepad
OUT=dist/obsidilight-$VERSION-windows-x86_64.zip
rm -f "$OUT" && (cd "$(dirname "$Z")" && zip -qr9 - "$(basename "$Z")") > "$OUT"
ls -l "$OUT"
