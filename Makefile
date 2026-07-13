# Proteus — developer convenience targets.
# Run `make install-hooks` once after cloning to wire up the git hooks.

.PHONY: install-hooks check fmt clippy test build-web serve-web

## Wire up the git hooks from scripts/git-hooks/ into .git/hooks/.
install-hooks:
	cp scripts/git-hooks/pre-push .git/hooks/pre-push
	chmod +x .git/hooks/pre-push
	@echo "✓ git hooks installed"

## Run the same checks that CI runs (fmt + clippy + tests).
check: fmt clippy test

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all

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
