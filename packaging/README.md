# Packaging obsidilight

One command per platform; every package lands in `dist/` (gitignored):

| Command | Output | Built |
|---|---|---|
| `make arch` | `dist/obsidilight-<ver>-<rel>-x86_64.pkg.tar.zst` | natively (makepkg) |
| `make deb` | `dist/obsidilight_<ver>-1_amd64.deb` | in a Debian 12 container |
| `make windows` | `dist/obsidilight-<ver>-windows-x86_64.zip` | cross-compiled in a container; natively on Windows |
| `make dist` | all three | |

`make test` runs the unit and fixture tests. `make test-all` adds the PDF export of every
fixture note and the scripted UI tests, which need a display. `make help` lists all targets.

The version comes from `Cargo.toml` (`[workspace.package] version`). The app is one
self-contained binary: the MathJax/CM6 JS bundles and the UI fonts are compiled in
(`include_str!`/`include_bytes!`), so every package only adds a desktop entry, an icon and
the licenses.

## Host requirements (Linux)

- Rust via rustup. `rust-toolchain.toml` pins the version.
- Node.js + npm, only to build the JS bundles once (`make bundles`, see `tools/js-bundles/`).
  They are gitignored, so a fresh clone needs this before the first build.
- `base-devel` for `make arch` (makepkg, Arch hosts only).
- `podman` for `make deb` / `make windows`. The build images are created on first use
  (`packaging/*/Containerfile`, a few minutes) and cached after that. Cargo downloads are cached
  in the `obsidilight-cargo-registry` volume, and the build outputs go to `target/cross/<platform>/`.
  `CONTAINER=docker` also works, but rootful docker leaves root-owned files in `target/` and `dist/`.

## Arch Linux — `packaging/arch/`

`build.sh` copies the release binary, `.desktop`, icon and licenses into `target/pkg/arch/`.
It writes `pkgver` (from Cargo.toml) and the checksums into a copy of `PKGBUILD` and runs makepkg
there. Nothing needs to be edited by hand; bump `pkgrel` in `PKGBUILD` only to
re-release the same version. Runtime dependencies and what each one is for: `arch/DEPENDENCIES.txt`.

Copy **only** `dist/obsidilight-<ver>-<rel>-x86_64.pkg.tar.zst` to the target machine and install it:

```sh
sudo pacman -U obsidilight-*-x86_64.pkg.tar.zst
```

Nothing under `target/` is installable. (An earlier version staged an
`obsidilight-<ver>-linux-x86_64.tar.zst` there. `pacman -U` on that file fails with
"missing package metadata", because it is a plain tarball and not a package.)

## Debian / Ubuntu — `packaging/debian/`

The Arch-built binary needs glibc 2.44 and runs on no Debian or Ubuntu release, so the `.deb` is
compiled inside `debian:bookworm`. The resulting binary needs glibc ≥ 2.35, so it installs on
Debian 12+, Ubuntu 22.04+ and their derivatives. `build.sh` gets the linked-library
dependencies from `dpkg-shlibdeps`, adds the dlopen'd ones (EGL, Vulkan, Wayland), and
writes `copyright` (MIT + font licenses) and `changelog.Debian.gz`. Build on another base with
`make deb DEB_IMAGE=docker.io/library/ubuntu:22.04`.

```sh
sudo apt install ./dist/obsidilight_*_amd64.deb
```

## Windows — `packaging/windows/`

A portable zip: `obsidilight.exe` (GUI subsystem, icon embedded, static runtime, imports only
Windows system DLLs) + licenses + `README.txt`. Requires Windows 10 1903+ and a DirectX 11 GPU.

**Cross-compiling from Linux (`make windows`).** Target `x86_64-pc-windows-gnu`, linked with
mingw-w64, in an Arch-based container. The hard part is GPUI's DirectX shaders. Upstream's
build script precompiles them with the Windows SDK's `fxc.exe`, but only when the build *host*
is Windows. A cross build skips that step and then fails on the missing `shaders_bytes.rs`.
`build.sh` therefore patches in a copy of `gpui-pre-windows` whose build script
(`gpui-windows-build.rs`) compiles the same shaders (same entry points, `vs_4_1`/`ps_4_1`) with
**vkd3d-compiler**, an open-source HLSL → DXBC compiler. The patch is applied with
`cargo --config patch.crates-io...` only for this build, and the committed `Cargo.lock` is
restored afterwards. It follows whatever gpui-pre-windows version the lockfile pins.

**Native build on Windows (`packaging\windows\build.ps1`).** This is the normal MSVC
toolchain, and upstream's build script uses the real `fxc.exe`. On a fresh clone:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install OpenJS.NodeJS.LTS        # only for the JS bundles
powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
```

The script checks the prerequisites, builds the JS bundles if they are missing, and writes
`dist\obsidilight-<ver>-windows-x86_64.zip` (`-NoZip` builds only the exe). With GNU make
installed, `make windows` on Windows runs the same script.

**Testing with Wine does not work.** The exe starts and every DLL import resolves, but GPUI's
text system needs `IDWriteFontSet::GetMatchingFonts`. That is a stub in Wine 11, so font
resolution fails for every family and the app panics before its first frame. Test on real
Windows or in a VM.

## Shared files — `packaging/common/`

`obsidilight.desktop` (menu entry; `Exec=obsidilight %f`, so a launch from the menu reopens the
last vault, or shows the welcome page) and `obsidilight.png` (64×64 icon). The Windows icon
`crates/app/assets/obsidilight.ico` is generated from the PNG:
`magick packaging/common/obsidilight.png -define icon:auto-resize=256,64,48,32,16 crates/app/assets/obsidilight.ico`.
