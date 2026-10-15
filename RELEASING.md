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
  can't be published twice, so running the workflow again would fail at the first of them. Fix
  the cause, then [publish the rest by hand](#publishing-the-rest-by-hand), or release a patch
  version with the fix.
- **crates.io answers "429 Too Many Requests"** for a crate published for the first time: an
  account can publish five new crates at once, then about one every ten minutes, and the message
  says when to try again. It only happens when a release adds new crates, since new versions of
  existing crates have a much higher limit. v0.1.0, which published all eight for the first
  time, stopped after five. Wait, then [publish the rest by hand](#publishing-the-rest-by-hand).
- **A bad version is out:** crates.io can't delete a version, only yank it
  (`cargo yank --version 0.2.0 <crate>`), which stops new projects from choosing it. npm allows
  `npm unpublish proteus-sdk@0.2.0` within 72 hours, and `npm deprecate` after that. Either way,
  release a fixed patch version.

#### Publishing the rest by hand

1. On crates.io, under Account Settings → API Tokens, create a token with the `publish-new` and
   `publish-update` scopes, limited to the crates `proteus-*`, that expires soon.
2. Check out the release's tag, and give Cargo the token; `cargo login` asks for it:

   ```bash
   git checkout v0.2.0
   cargo login
   ```

3. Publish each crate that isn't on crates.io yet, in this order: `proteus-gpu`,
   `proteus-render`, `proteus-ui`, `proteus-sdk`, `proteus-runtime`, `proteus-sdk-web`,
   `proteus-host-winit`, then `proteus-host-web`, which only builds for WebAssembly. For example:

   ```bash
   cargo publish -p proteus-host-winit
   cargo publish -p proteus-host-web --target wasm32-unknown-unknown
   ```

4. Remove the token from your machine with `cargo logout`, and delete it on crates.io.
5. The workflow creates the GitHub Release only once both publish jobs succeed, so create it by
   hand, with the version's section of `CHANGELOG.md` as its notes:

   ```bash
   awk -v heading="## 0.2.0 (" 'index($0, heading) == 1 { found = 1; next }
     found && /^## / { exit } found { print }' CHANGELOG.md > notes.md
   gh release create v0.2.0 --verify-tag --title v0.2.0 --notes-file notes.md
   ```

### Publishing credentials

`release.yml` publishes with **trusted publishing**: crates.io and npm trust this repository's
`release.yml`, running in the `release` environment, through GitHub's OIDC token, so no
long-lived token is stored anywhere. Each crate is linked in its Settings → Trusted Publishing
on crates.io, and the npm package in its Settings → Trusted publisher, with repository
`ichellin1/proteus`, workflow `release.yml` and environment `release`.

On npm, a trusted publisher must be used to publish within 48 hours of being added, or it
lapses. If npm shows it as lapsed, or still "Pending validation" past its date, add it again
just before tagging the next release, so that release's publish validates it.

**A release that adds a new crate or package** needs a token for it, since trusted publishing
can only be linked to a package that already exists. `release.yml` uses a token whenever its
secret is set:

1. Before tagging, add the repository secret (Settings → Secrets and variables → Actions):
   `CARGO_REGISTRY_TOKEN`, a crates.io API token with the `publish-new` and `publish-update`
   scopes, or `NPM_TOKEN`, an npm granular access token that can publish.
2. Release as usual. Remember crates.io's limit on new crates; see
   [If something fails](#if-something-fails).
3. Link trusted publishing for the new crate or package, as above.
4. Delete the secret, and the token on crates.io or npm.

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
4. **Merge** once CI passes and the preview looks right, with a merge commit, on GitHub or
   locally (see [CONTRIBUTING.md](./CONTRIBUTING.md#pull-requests)). Pushing the merge to
   `main` deploys the demo and the API reference. Within a few minutes, check <https://ichellin1.github.io/proteus/>.

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
