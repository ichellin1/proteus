# Releasing the Reference Demo (Web)

How to ship a change to the web build of the reference demo, viewed at
**<https://ichellin1.github.io/proteus/>**. Four steps: review locally, open a PR, review on
staging, merge.

*Native has no release path — it's `cargo run -p
proteus-shell-native` from source (see [GETTING_STARTED.md](./GETTING_STARTED.md)), nothing
gets deployed.*

## 1. Review locally

See [GETTING_STARTED.md](./GETTING_STARTED.md) for one-time dependency setup (Rust, wasm-pack,
Python 3). Then, from the repo root:

```bash
make build-web
make serve-web
```

Open <http://localhost:8080> and walk through the app looking for regressions.

## 2. Open a pull request

```bash
git checkout -b my-change
git add -A
git commit -m "..."
git push -u origin my-change
gh pr create --fill
```

(Or push the branch and click "Compare & pull request" on GitHub if you don't use `gh`.)

This kicks off `ci.yml` (formatting, lints, tests, wasm builds) on the PR. Wait for it to pass:

```bash
gh pr checks <PR-number>
```

or verify it is green on GitHub.

## 3. Review on staging

If the PR touches anything under `crates/` (or `Cargo.toml`/`Cargo.lock`), a second workflow
automatically builds and deploys a live preview — a real hosted URL, not localhost — to
`https://ichellin1.github.io/proteus-staging/pr-<number>/`, and posts that URL as a comment on
the PR. Open it and repeat the walkthrough from step 1 against the actual deployed build. It
rebuilds on every push to the PR and tears itself down when the PR closes.

(The filter is deliberately the whole workspace rather than the handful of crates the web
build reads most directly — every crate under `crates/` either compiles into the deployed wasm
or is cheap to rebuild, and a narrower list is one refactor away from silently skipping a
deploy. A docs- or workflow-only PR gets no preview, which is the intent.)

## 4. Merge to main

Once CI is green and the staging preview looks right:

```bash
gh pr merge <PR-number> --squash
```

(Or the "Merge pull request" button on GitHub.) The merge itself triggers the production
deploy — nothing further to run. Give it a minute or two, then open
<https://ichellin1.github.io/proteus/> and confirm the change is live.

## How CI and the workflow files facilitate this

Three workflows under `.github/workflows/`, each doing one job in the flow above:

- **`ci.yml`** — runs on every push and PR: `cargo fmt`/`clippy`/`doc`/`test` plus wasm builds
  and the TypeScript SDK's own type-check, package check and example build. The gate step 2
  waits on before a PR is mergeable.
- **`staging-deploy.yml`** — runs on PR open/update/close for the paths above: builds the web
  shell, publishes it to a per-PR subpath on a separate repo (`ichellin1/proteus-staging`, so a
  preview can never touch production), comments the URL, and removes it when the PR closes.
  This is what step 3 reviews.
- **`pages.yml`** — runs on every push to `main` that touches those same paths (plus a manual
  `workflow_dispatch` trigger, for redeploying without a new commit, or after a docs-only
  change that the path filter skipped): builds the web shell and deploys it to GitHub Pages.
  This is what step 4's merge triggers.

Rolling back is the same shape in reverse: `git revert` the offending commit(s) on `main` and
push (or run `gh workflow run pages.yml` to redeploy without a new commit).
