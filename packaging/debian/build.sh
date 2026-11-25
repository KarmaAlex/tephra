#!/usr/bin/env bash
# Builds tephra and packs dist/tephra_<version>-<rev>_amd64.deb. Runs INSIDE the
# tephra-build-debian container (packaging/debian/Containerfile); started by `make deb`.
set -euo pipefail

REV=${DEB_REVISION:-1}
export CARGO_TARGET_DIR=target/cross/debian
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --locked --release -p tephra
BIN=$CARGO_TARGET_DIR/release/tephra

# --- file tree -------------------------------------------------------------------------------
ROOT=$CARGO_TARGET_DIR/deb/root
rm -rf "$ROOT" && mkdir -p "$ROOT/DEBIAN"
install -Dm755 "$BIN" "$ROOT/usr/bin/tephra"
install -Dm644 packaging/common/tephra.desktop "$ROOT/usr/share/applications/tephra.desktop"
install -Dm644 packaging/common/tephra.png "$ROOT/usr/share/icons/hicolor/64x64/apps/tephra.png"
DOC=$ROOT/usr/share/doc/tephra
install -d "$DOC"
{
    echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
    echo "Upstream-Name: tephra"
    echo
    echo "Files: *"
    echo "License: MIT"
    sed 's/^$/./; s/^/ /' LICENSE
    echo
    echo "Files: crates/app/assets/fonts/IBMPlexSans-*"
    echo "License: OFL-1.1"
    sed 's/^$/./; s/^/ /' crates/app/assets/fonts/LICENSE-IBMPlexSans.txt
    echo
    echo "Files: crates/app/assets/fonts/DejaVu*"
    echo "License: Bitstream-Vera"
    sed 's/^$/./; s/^/ /' crates/app/assets/fonts/LICENSE-DejaVu.txt
} > "$DOC/copyright"
printf 'tephra (%s-%s) unstable; urgency=medium\n\n  * Package build.\n\n -- tephra packaging <noreply@localhost>  %s\n' \
    "$VERSION" "$REV" "$(date -R)" | gzip -9n > "$DOC/changelog.Debian.gz"

# --- dependencies ----------------------------------------------------------------------------
# Linked libraries: computed by dpkg-shlibdeps from the binary (needs a stub debian/control).
# dlopen'd at runtime (invisible to shlibdeps): EGL, Vulkan loader, Wayland client/EGL.
work=$CARGO_TARGET_DIR/deb/shlibdeps && rm -rf "$work" && mkdir -p "$work/debian"
printf 'Source: tephra\n\nPackage: tephra\nArchitecture: amd64\n' > "$work/debian/control"
linked=$(cd "$work" && dpkg-shlibdeps -O "$OLDPWD/$ROOT/usr/bin/tephra" | sed -n 's/^shlibs:Depends=//p')
DEPENDS="$linked, libegl1, libvulkan1, libwayland-client0, libwayland-egl1"

cat > "$ROOT/DEBIAN/control" <<EOF
Package: tephra
Version: $VERSION-$REV
Architecture: amd64
Maintainer: tephra packaging <noreply@localhost>
Installed-Size: $(du -sk --exclude=DEBIAN "$ROOT" | cut -f1)
Depends: $DEPENDS
Recommends: mesa-vulkan-drivers | vulkan-icd, xdg-desktop-portal, fonts-ibm-plex, fonts-dejavu-core
Suggests: hunspell-en-us | hunspell-dictionary
Section: editors
Priority: optional
Description: Lightweight Obsidian-compatible notes app
 A native (GPUI) Markdown notes editor that opens Obsidian vaults: wiki links,
 embeds, LaTeX math, tasks, daily notes and PDF export, without Electron.
 The UI fonts are embedded in the binary; the font packages are only used as
 system-side glyph fallback. xdg-desktop-portal provides the vault folder picker.
EOF

mkdir -p dist
OUT=dist/tephra_${VERSION}-${REV}_amd64.deb
dpkg-deb --root-owner-group -Zxz --build "$ROOT" "$OUT"
dpkg-deb --info "$OUT" | sed -n '/Package:/,$p'
echo "glibc required: $(objdump -T "$BIN" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1)"
ls -l "$OUT"
