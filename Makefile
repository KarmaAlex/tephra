# obsidilight — developer entry points
VAULT ?= $(HOME)/dev/vault-fixture

.PHONY: perf perf-obsidian corpus-math vault-stats bench-ref spikes mathjax-bundle cm6-bundle

perf:            ## idle memory / startup / typing for the spikes (add OBSIDIAN=1 for the baseline)
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

mathjax-bundle:  ## MathJax SVG bundle for the GPUI spike's embedded QuickJS
	cd tools/corpus && ../../spikes/tauri/node_modules/.bin/esbuild mathjax-entry.js --bundle --minify --format=iife --platform=neutral --main-fields=module,main --target=es2020 '--define:PACKAGE_VERSION="3.2.1"' '--banner:js=var require=function(m){throw new Error("require("+m+") unavailable")};var __dirname="/";' --outfile=../../spikes/gpui/assets/mathjax-svg.js

cm6-bundle:      ## headless CM6 + Lezer Markdown bundle for QuickJS (snippet function bridge)
	cd spikes/gpui/js && ../../tauri/node_modules/.bin/esbuild cm6-entry.js --bundle --minify --format=iife --platform=neutral --main-fields=module,main --target=es2020 --outfile=../assets/cm6-headless.js
