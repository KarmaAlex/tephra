# obsidilight — developer entry points. `make help` lists them.
VAULT ?= $(HOME)/dev/vault-fixture
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
RUST_TOOLCHAIN := $(shell sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)

# Container builds (Debian .deb, Windows cross-compile): podman (rootless) by default.
CONTAINER ?= podman
DEB_IMAGE ?= docker.io/library/debian:bookworm
CARGO_CACHE ?= obsidilight-cargo-registry
RUN_IN = $(CONTAINER) run --rm -v "$(CURDIR)":/src:z -w /src -v $(CARGO_CACHE):/opt/cargo/registry

# JS bundles the app embeds with include_str! (gitignored; built once, `make bundles` rebuilds)
BUNDLES := crates/app/assets/mathjax-svg.js crates/app/assets/cm6-headless.js

.DEFAULT_GOAL := build
.PHONY: help build bundles mathjax-bundle cm6-bundle test test-all export-check ui-smoke \
        dist arch deb windows windows-native clean-dist \
        perf perf-obsidian corpus-math vault-stats bench-ref spikes

help:            ## list the targets
	@grep -hE '^[a-z][a-z-]*:.*## ' $(MAKEFILE_LIST) | sed 's/:[^#]*## /\t/' | expand -t18

# --- build -----------------------------------------------------------------------------------------
build: $(BUNDLES) ## release build of the app (+ winshot, the UI tests' screenshot tool)
	cargo build --release -p obsidilight -p winshot

$(BUNDLES) &:
	$(MAKE) bundles

bundles:         ## (re)build the embedded JS bundles: MathJax SVG + headless CM6 (needs Node.js)
	cd tools/js-bundles && npm ci --no-audit --no-fund && npm run -s build
	if [ -d spikes/gpui/assets ]; then cp crates/app/assets/mathjax-svg.js spikes/gpui/assets/; fi

mathjax-bundle cm6-bundle: bundles

# --- tests -----------------------------------------------------------------------------------------
test: $(BUNDLES) ## unit + fixture tests (fixture tests skip if the fixture is absent)
	cargo test --release --workspace

test-all: test export-check ui-smoke ## every test: unit + fixture, PDF export of every fixture note, scripted UI (needs a display)
	@echo "test-all: OK"

export-check: $(BUNDLES) ## every fixture note converts to Typst and compiles to PDF (~1 min)
	cargo test --release -p obsidilight fixture_notes_export -- --ignored --nocapture

ui-smoke: build  ## scripted UI tests (need a display; windows open briefly)
	for t in smoke links watcher idle snippets assist academic m4 tasks hotkeys panes sync toolbar pdf daily composer recovery preview graph theme files focus toolbar-layout move firstline caret emptyfolder fonts vaults; do tests/ui/$$t.sh || exit 1; done

# --- packages (all land in dist/) -------------------------------------------------------------------
dist: arch deb windows ## all packages: Arch .pkg.tar.zst, Debian/Ubuntu .deb, Windows .zip
	@ls -l dist/*$(VERSION)*

arch: build      ## Arch package (native makepkg)       -> dist/obsidilight-<ver>-<rel>-x86_64.pkg.tar.zst
	packaging/arch/build.sh

deb: $(BUNDLES)  ## Debian/Ubuntu package (in a container) -> dist/obsidilight_<ver>-1_amd64.deb
	$(CONTAINER) build -q --build-arg BASE_IMAGE=$(DEB_IMAGE) --build-arg RUST_TOOLCHAIN=$(RUST_TOOLCHAIN) \
		-t obsidilight-build-debian -f packaging/debian/Containerfile packaging/debian
	$(RUN_IN) obsidilight-build-debian packaging/debian/build.sh

ifeq ($(OS),Windows_NT)
windows: windows-native ## Windows .zip -> dist/obsidilight-<ver>-windows-x86_64.zip (cross-built in a container; native on Windows)
else
windows: $(BUNDLES)
	$(CONTAINER) build -q --build-arg RUST_TOOLCHAIN=$(RUST_TOOLCHAIN) \
		-t obsidilight-build-windows -f packaging/windows/Containerfile packaging/windows
	$(RUN_IN) obsidilight-build-windows packaging/windows/build.sh
endif

windows-native:  ## on a Windows machine: MSVC build via packaging/windows/build.ps1
	powershell -NoProfile -ExecutionPolicy Bypass -File packaging/windows/build.ps1

clean-dist:      ## remove built packages and packaging work dirs (not the cargo build caches)
	rm -rf dist target/pkg target/cross/*/deb target/cross/*/zip

# --- measurements & research tools -----------------------------------------------------------------
perf: build      ## idle memory / startup / typing for the app (OBSIDIAN=1 adds the baseline, SPIKES=1 the M0 spikes)
	VAULT="$(VAULT)" tools/perf/run_perf.sh

perf-obsidian:   ## same, including Obsidian on the fixture (isolated profile)
	OBSIDIAN=1 VAULT="$(VAULT)" tools/perf/run_perf.sh

corpus-math:     ## HANDOFF §7 corpus math test (details -> reports/, gitignored)
	cd tools/corpus && node --import tsx math.ts "$(VAULT)" --engine both --hybrid

vault-stats:     ## HANDOFF §5 figures (aggregates only)
	cd tools/corpus && node --import tsx stats.ts "$(VAULT)"

bench-ref:       ## validate benchmark hidden tests against reference solutions
	python3 bench/run_bench.py --score-ref

spikes:
	cd spikes/tauri && npm run -s build:fe && cd src-tauri && cargo build --release
	cd spikes/gpui && cargo build --release
