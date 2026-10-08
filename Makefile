# Proteus — developer convenience targets.
# Run `make install-hooks` once after cloning to wire up the git hooks.

.PHONY: install-hooks check fmt clippy test check-comments build-web serve-web build-sdk-web

## Wire up the git hooks from scripts/git-hooks/ into .git/hooks/.
install-hooks:
	cp scripts/git-hooks/pre-push .git/hooks/pre-push
	chmod +x .git/hooks/pre-push
	@echo "✓ git hooks installed"

## Run the same checks that CI runs (fmt + clippy + tests).
check: fmt clippy test

fmt:
	cargo fmt --all -- --check

## proteus-shell-web / proteus-host-web are wasm32-only (wgpu::SurfaceTarget::
## Canvas is #[cfg(web)]-gated), so both are excluded from the host-target
## pass and checked separately against their real target. Mirrors ci.yml.
clippy:
	cargo clippy --workspace --exclude proteus-shell-web --exclude proteus-host-web --all-targets --all-features -- -D warnings
	cargo clippy -p proteus-shell-web -p proteus-host-web -p proteus-docs -p gallery --target wasm32-unknown-unknown --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --exclude proteus-shell-web --exclude proteus-host-web

## Check code comments against CONTRIBUTING.md. Pass paths with PATHS=...; the default is
## every crate and example.
check-comments:
	scripts/check-comments.sh $(PATHS)

## Build the WebGL2 WASM demo with wasm-pack.
## Requires: cargo install wasm-pack
build-web:
	wasm-pack build crates/proteus-shell-web \
	  --target web \
	  --out-dir www/pkg \
	  --release

## Serve the web demo locally (requires Python 3).
serve-web: build-web
	python3 -m http.server 8080 --directory crates/proteus-shell-web/www

## Build the proteus-sdk npm package: wasm-pack (--target bundler, unlike
## build-web's --target web — this ships as an npm-installable package for
## bundler-based projects, not a zero-build-step demo page) for both
## proteus-sdk-web (the headless bridge, ts/pkg) and proteus-host-web (the
## mount() host, ts/pkg-host — M13.2) then tsc.
## Requires: cargo install wasm-pack; npm install (once) in crates/proteus-sdk-web/ts
build-sdk-web:
	wasm-pack build crates/proteus-sdk-web \
	  --target bundler \
	  --out-dir ts/pkg \
	  --release
	wasm-pack build crates/proteus-host-web \
	  --target bundler \
	  --out-dir ../proteus-sdk-web/ts/pkg-host \
	  --release
	cd crates/proteus-sdk-web/ts && npm run build

## The built proteus-sdk package, which the TypeScript examples use. It is
## rebuilt when any crate's Rust, the SDK's TypeScript or Cargo.lock changed
## since the last build.
SDK_BUILT := crates/proteus-sdk-web/ts/dist/index.js
SDK_SOURCES := $(shell find crates \( -name target -o -name node_modules -o -name pkg -o -name pkg-host \) -prune \
  -o \( -name '*.rs' -o -name Cargo.toml \) -print) \
  $(wildcard crates/proteus-sdk-web/ts/src/*.ts) Cargo.lock

$(SDK_BUILT): $(SDK_SOURCES)
	cd crates/proteus-sdk-web/ts && npm install
	$(MAKE) build-sdk-web

## Run a TypeScript example in a browser, such as `make example-gallery-ts`:
## builds the SDK if needed, installs the example's packages and starts Vite.
example-%-ts: $(SDK_BUILT)
	cd examples/$*/typescript && npm install && npm run dev

## Run a Rust example in a browser, such as `make example-gallery-web`: builds
## it for the web with wasm-pack, then serves it on http://localhost:8080.
## Requires Python 3, for the server.
example-%-web:
	wasm-pack build examples/$*/rust --target web --release
	python3 -m http.server 8080 --directory examples/$*/rust
