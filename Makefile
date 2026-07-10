# Proteus — developer convenience targets.
# Run `make install-hooks` once after cloning to wire up the git hooks.

.PHONY: install-hooks check fmt clippy test

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
