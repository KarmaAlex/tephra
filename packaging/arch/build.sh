#!/usr/bin/env bash
# Builds dist/obsidilight-<version>-<pkgrel>-x86_64.pkg.tar.zst on an Arch host (`make arch`).
# Needs: base-devel (makepkg). The release binary must already be built (`make build`).
set -euo pipefail
cd "$(dirname "$0")/../.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BIN=${BIN:-target/release/obsidilight}
[[ -x $BIN ]] || { echo "missing $BIN -- run: make build" >&2; exit 1; }
W=target/pkg/arch
rm -rf "$W" && mkdir -p "$W/obsidilight-$VERSION" dist

# source tarball: binary + desktop entry + icon + licenses
S=$W/obsidilight-$VERSION
cp "$BIN" "$S/obsidilight"
cp packaging/common/obsidilight.desktop packaging/common/obsidilight.png LICENSE "$S/"
cp crates/app/assets/fonts/LICENSE-IBMPlexSans.txt crates/app/assets/fonts/LICENSE-DejaVu.txt "$S/"
tar -C "$W" -I 'zstd -T0 -q' -cf "$W/obsidilight-$VERSION-linux-x86_64.tar.zst" "obsidilight-$VERSION"
rm -rf "$S"

sum=$(sha256sum "$W/obsidilight-$VERSION-linux-x86_64.tar.zst" | cut -d' ' -f1)
sed -e "s/^pkgver=.*/pkgver=$VERSION/" -e "s/^sha256sums=.*/sha256sums=('$sum')/" \
    packaging/arch/PKGBUILD > "$W/PKGBUILD"

# --nodeps: this only repacks a binary, the runtime deps need not be installed on the build host
(cd "$W" && PKGDEST="$PWD/../../../dist" makepkg -f --nodeps --noconfirm)
pkg=$(cd "$W" && PKGDEST="$PWD/../../../dist" makepkg --packagelist)
pacman -Qip "$pkg" | grep -E '^(Name|Version|Depends On|Installed Size)'
ls -l "$pkg"
