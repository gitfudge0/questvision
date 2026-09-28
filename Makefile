CARGO ?= cargo
ARGS ?=
PRESET ?= balanced

.DEFAULT_GOAL := help
.PHONY: help build release app app-run run dev doctor displays config devices benchmark check test fmt fmt-check lint verify clean

help:
	@printf '%s\n' \
	  'Usage: make <target> [CARGO=cargo] [ARGS="..."] [PRESET=balanced]' \
	  '' \
	  'Targets:' \
	  '  help        Show this help (default)' \
	  '  build       Build the debug binary' \
	  '  release     Build the optimized binary' \
	  '  app         Build a locally signed macOS app bundle' \
	  '  app-run     Build and launch the macOS app with terminal pairing; accepts ARGS' \
	  '  run         Start the optimized host (app bundle on macOS); accepts ARGS' \
	  '  dev         Start the debug host; accepts ARGS' \
	  '  doctor      Run host diagnostics; accepts ARGS' \
	  '  displays    List displays; accepts ARGS' \
	  '  config      Show configuration; accepts ARGS' \
	  '  devices     List or manage devices; accepts ARGS' \
	  '  benchmark   Benchmark capture/encoding; PRESET=performance|balanced|quality, accepts ARGS' \
	  '  check       Check all targets' \
	  '  test        Run tests; accepts ARGS' \
	  '  fmt         Format Rust code' \
	  '  fmt-check   Check Rust formatting' \
	  '  lint        Run Clippy with warnings denied' \
	  '  verify      Run fmt-check, lint, test, release in order' \
	  '  clean       Remove Cargo build output'

build:
	$(CARGO) build

release:
	$(CARGO) build --release

app:
	@test "$$(uname -s)" = Darwin || { printf '%s\n' 'make app requires macOS.' >&2; exit 1; }
	$(MAKE) release
	sh scripts/package-macos-app.sh 'target/release/questdisplay' 'target/release/Quest Display.app'

ifeq ($(shell uname -s),Darwin)
run:
	$(MAKE) app-run
else
run:
	$(CARGO) run --release -- start $(ARGS)
endif

app-run:
	@test "$$(uname -s)" = Darwin || { printf '%s\n' 'make app-run requires macOS.' >&2; exit 1; }
	$(MAKE) app
	bash scripts/run-macos-app.sh '$(CURDIR)/target/release/Quest Display.app' start $(ARGS)

dev:
	$(CARGO) run -- start $(ARGS)

doctor:
	$(CARGO) run --release -- doctor $(ARGS)

displays:
	$(CARGO) run --release -- displays $(ARGS)

config:
	$(CARGO) run --release -- config $(ARGS)

devices:
	$(CARGO) run --release -- devices $(ARGS)

benchmark:
	$(CARGO) run --release -- benchmark --preset $(PRESET) $(ARGS)

check:
	$(CARGO) check --all-targets

test:
	$(CARGO) test $(ARGS)

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --all-targets -- -D warnings

verify:
	$(MAKE) fmt-check
	$(MAKE) lint
	$(MAKE) test
	$(MAKE) release

clean:
	$(CARGO) clean
