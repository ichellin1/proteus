# Releasing

Two things ship from this repository:

- **The library:** eight crates on crates.io and the `proteus-sdk` package on npm, all at one
  version. A tag releases them; see [Releasing the library](#releasing-the-library).
- **The reference demo and the API reference,** on GitHub Pages at
  <https://ichellin1.github.io/proteus/>. Merging into `main` deploys them; see
  [Deploying the demo](#deploying-the-demo).

## Releasing the library

The published crates are `proteus-gpu`, `proteus-render`, `proteus-ui`, `proteus-sdk`,
`proteus-runtime`, `proteus-host-winit`, `proteus-host-web` and `proteus-sdk-web`. The demo, its
shells, the examples and `proteus-docs` are `publish = false`.

### Choosing the version

Proteus follows [Semantic Versioning](https://semver.org). Before 1.0, a release that breaks
existing code raises the minor version (0.1 to 0.2), and any other release raises the patch
version (0.1.0 to 0.1.1). A commit marked breaking (`feat!:`, `fix!:`) means the next release
breaks existing code.

### 1. Set the version in a pull request

```bash
git checkout -b release/v0.2.0 main
node scripts/version.mjs set 0.2.0
```

`version.mjs set` changes the version everywhere: `Cargo.toml` and `Cargo.lock`, the npm package
and the TypeScript examples, and the version requirements in the docs, such as
`proteus-sdk = "0.2"`. It then regenerates `CHANGELOG.md` with that version's section, from the
commits since the last release; it needs `git-cliff` (`cargo install git-cliff`).

Read the new section of `CHANGELOG.md`: it becomes the GitHub Release's notes. A line that reads
badly comes from a commit message; the changelog is never edited by hand, but a commit can be
left out of it in `cliff.toml`. Then commit and open a pull request:

```bash
git commit -am "chore(release): v0.2.0"
git push -u origin release/v0.2.0
gh pr create --fill
```

### 2. Dry run

On GitHub, under Actions → Release → Run workflow, pick the release branch and enter the version.
This runs every step of the release without publishing anything: it checks the version, runs CI,
runs `cargo publish --dry-run` for every crate and `npm publish --dry-run`, and creates a draft
GitHub Release with the notes. Check the draft, then delete it.

### 3. Merge, then tag

Merge the pull request once CI and the dry run pass. Then tag the merge on `main`:

```bash
git checkout main
git pull
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

### 4. Approve

The tag starts `release.yml`. It checks that the tag matches the version everywhere, runs CI, and
then waits for approval: both publish jobs use the `release` environment, whose required
reviewer approves them under the run's "Review deployments". Approval publishes the crates, in
dependency order, and the npm package. Once both are published, it creates the GitHub Release.

### 5. Check

- The crates on crates.io, and their docs on docs.rs, which builds them within the hour.
- The package on npm.
- The GitHub Release, with the changelog's section as its notes.

### If something fails

Nothing is published until both checks and CI pass, and nothing until approval. After that:

- **A crate fails to publish:** the crates before it are already on crates.io, and a version
  can't be published twice. Fix the cause, then publish the rest by hand
  (`cargo publish -p <crate>`), or release a patch version with the fix.
- **A bad version is out:** crates.io can't delete a version, only yank it
  (`cargo yank --version 0.2.0 <crate>`), which stops new projects from choosing it. npm allows
  `npm unpublish proteus-sdk@0.2.0` within 72 hours, and `npm deprecate` after that. Either way,
  release a fixed patch version.

### Publishing credentials

`release.yml` publishes with **trusted publishing**: crates.io and npm trust this repository's
`release.yml`, running in the `release` environment, through GitHub's OIDC token, so no
long-lived token is stored. A package must exist before trusted publishing can be linked to it,
so the first release, v0.1.0, publishes with tokens instead:

1. **Before v0.1.0,** add two repository secrets (Settings → Secrets and variables → Actions):
   `CARGO_REGISTRY_TOKEN`, a crates.io API token with the `publish-new` and `publish-update`
   scopes, and `NPM_TOKEN`, an npm granular access token that can publish. `release.yml` uses a
   token when its secret is set.
2. **After v0.1.0,** link trusted publishing: on crates.io, in each crate's Settings → Trusted
   Publishing, and on npm, in the package's Settings → Trusted publisher. In each, give
   repository `ichellin1/proteus`, workflow `release.yml` and environment `release`.
3. **Then delete both secrets,** and the tokens on crates.io and npm.

### The changelog over time

`CHANGELOG.md` is regenerated from the whole history at each release. When it grows long, such as
at 1.0, the older versions can move to a file of their own: generate them once with a fixed
range (`git-cliff v0.1.0..v1.0.0 -o changelog/0.x.md`), commit that file, and have
`version.mjs` generate `CHANGELOG.md` from `v1.0.0` on, with a link to it. Each GitHub Release
keeps its own notes either way.

## Deploying the demo

The demo, and the API reference beside it, deploy to GitHub Pages from `main`. A change reaches
them through a pull request.

1. **Check it locally.** `make serve-web` builds the web demo and serves it on
   <http://localhost:8080>; walk through it looking for regressions.
2. **Open a pull request.** CI runs on it. If it changes anything under `crates/`, or
   `Cargo.toml` or `Cargo.lock`, a preview of the web demo is built and deployed to
   `https://ichellin1.github.io/proteus-staging/pr-<number>/`, and its address is posted as a
   comment. It's rebuilt on every push, and removed when the pull request closes.
3. **Review the preview,** as in step 1, on the real deployed build.
4. **Merge** once CI passes and the preview looks right, with a merge commit (see
   [CONTRIBUTING.md](./CONTRIBUTING.md#pull-requests)). The merge deploys the demo and the API
   reference. Within a few minutes, check <https://ichellin1.github.io/proteus/>.

To undo a deploy, revert the commit on `main` and push. To deploy again without a new commit,
such as after a change the path filter skips, run the Pages workflow by hand:
`gh workflow run pages.yml`.

### The workflows

- **`ci.yml`** runs on every pull request and push to `main`: formatting, clippy, docs and tests,
  the WebAssembly builds, the TypeScript SDK's checks, the examples' builds, and a lint of the
  workflows. `release.yml` runs it too.
- **`staging-deploy.yml`** builds each pull request's preview and publishes it to a separate
  repository, `ichellin1/proteus-staging`, so a preview can never touch the real site.
- **`pages.yml`** builds the demo and the API reference on every push to `main` that touches
  `crates/`, `Cargo.toml` or `Cargo.lock`, and deploys them to GitHub Pages.
- **`release.yml`** releases the library; see above.
