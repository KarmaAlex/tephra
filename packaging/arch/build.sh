#!/usr/bin/env bash
# Builds dist/obsidilight-<version>-<pkgrel>-x86_64.pkg.tar.zst on an Arch host (`make arch`).
# Needs: base-devel (makepkg). The release binary must already be built (`make build`).
set -euo pipefail
cd "$(dirname "$0")/../.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BIN=${BIN:-target/release/obsidilight}
[[ -x $BIN ]] || { echo "missing $BIN -- run: make build" >&2; exit 1; }
W=target/pkg/arch
rm -rf "$W" && mkdir -p "$W" dist

# makepkg sources: binary + desktop entry + icon + licenses, next to a copy of the PKGBUILD
cp "$BIN" "$W/obsidilight"
cp packaging/common/obsidilight.desktop packaging/common/obsidilight.png LICENSE "$W/"
cp crates/app/assets/fonts/LICENSE-IBMPlexSans.txt crates/app/assets/fonts/LICENSE-DejaVu.txt "$W/"
sums=$(cd "$W" && sha256sum obsidilight obsidilight.desktop obsidilight.png LICENSE \
    LICENSE-IBMPlexSans.txt LICENSE-DejaVu.txt | awk '{printf "%s\x27%s\x27", (NR > 1 ? " " : ""), $1}')
sed -e "s/^pkgver=.*/pkgver=$VERSION/" -e "s/^sha256sums=.*/sha256sums=($sums)/" \
    packaging/arch/PKGBUILD > "$W/PKGBUILD"

# --nodeps: this only repacks a binary, the runtime deps need not be installed on the build host
(cd "$W" && PKGDEST="$PWD/../../../dist" makepkg -f --nodeps --noconfirm)
pkg=$(cd "$W" && PKGDEST="$PWD/../../../dist" makepkg --packagelist)
pacman -Qip "$pkg" | grep -E '^(Name|Version|Depends On|Installed Size)'
echo
echo "Package: ${pkg#"$PWD/"}"
echo "Install: sudo pacman -U $(basename "$pkg")   (copy only this file; nothing in target/ is installable)"
