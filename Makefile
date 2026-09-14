# Proteus — developer convenience targets.
# Run `make install-hooks` once after cloning to wire up the git hooks.

.PHONY: install-hooks check fmt clippy test build-web serve-web build-sdk-web

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
	cargo clippy -p proteus-shell-web -p proteus-host-web --target wasm32-unknown-unknown --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --exclude proteus-shell-web --exclude proteus-host-web

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
