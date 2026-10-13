#!/usr/bin/env node
// Sets, or checks, the one version that every crate and npm package shares.
//
//   node scripts/version.mjs set 0.2.0     sets it everywhere, and regenerates
//                                          CHANGELOG.md for v0.2.0
//   node scripts/version.mjs check 0.2.0   fails unless it's 0.2.0 everywhere
//
// `set` changes:
//   - Cargo.toml: the workspace version, and the version on each of the
//     workspace's own crates in [workspace.dependencies];
//   - Cargo.lock, through `cargo update --workspace`;
//   - the npm package and the TypeScript examples: package.json, and in
//     package-lock.json their own version and the SDK version they link to;
//   - the version requirements in the docs and crate READMEs, such as
//     `proteus-sdk = "0.1"`;
//   - CHANGELOG.md, with git-cliff, which must be installed.
// The result is reviewed and merged as a normal pull request; see RELEASING.md.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const [command, version] = process.argv.slice(2);

if (!["set", "check"].includes(command) || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("usage: node scripts/version.mjs set|check X.Y.Z");
  process.exit(2);
}

// What a dependent writes in its Cargo.toml: "0.2" for 0.2.x, "1" for 1.x.
const [major, minor] = version.split(".");
const requirement = major === "0" ? `0.${minor}` : major;

const problems = [];

/** Sets, or checks, one occurrence of a version in a file. */
function expect(file, what, found, wanted) {
  if (found === undefined) {
    problems.push(`${file}: ${what} not found`);
  } else if (found !== wanted) {
    problems.push(`${file}: ${what} is ${found}, not ${wanted}`);
  }
}

// --- Cargo.toml -----------------------------------------------------------

const cargoPath = join(root, "Cargo.toml");
let cargo = readFileSync(cargoPath, "utf8");
const workspaceVersion = /^\[workspace\.package\][^[]*?^version = "([^"]+)"/m;
const internal = /^(proteus-[a-z-]+) = \{ path = "crates\/[^"]+", version = "([^"]+)" \}$/gm;
if (command === "set") {
  cargo = cargo.replace(workspaceVersion, (all, old) => all.replace(`"${old}"`, `"${version}"`));
  cargo = cargo.replace(internal, (all, name, old) => all.replace(`version = "${old}"`, `version = "${version}"`));
  writeFileSync(cargoPath, cargo);
}
expect("Cargo.toml", "the workspace version", cargo.match(workspaceVersion)?.[1], version);
const internalCrates = [...cargo.matchAll(internal)];
if (internalCrates.length === 0) problems.push("Cargo.toml: no versioned workspace crates found");
for (const [, name, found] of internalCrates) {
  expect("Cargo.toml", `${name}'s version`, found, version);
}

// --- npm packages ---------------------------------------------------------

const sdk = "crates/proteus-sdk-web/ts";
const examples = readdirSync(join(root, "examples"), { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => `examples/${entry.name}/typescript`)
  .filter((dir) => existsSync(join(root, dir, "package.json")));

for (const dir of [sdk, ...examples]) {
  const pkgFile = `${dir}/package.json`;
  const pkg = JSON.parse(readFileSync(join(root, pkgFile), "utf8"));
  const lockFile = `${dir}/package-lock.json`;
  const lock = JSON.parse(readFileSync(join(root, lockFile), "utf8"));
  // An example's lockfile records the SDK it links to under its relative path.
  const sdkLink = relative(join(root, dir), join(root, sdk));
  const linked = dir === sdk ? undefined : lock.packages[sdkLink];
  if (dir !== sdk && !linked) problems.push(`${lockFile}: no entry for ${sdkLink}`);

  if (command === "set") {
    pkg.version = version;
    lock.version = version;
    lock.packages[""].version = version;
    if (linked) linked.version = version;
    writeFileSync(join(root, pkgFile), `${JSON.stringify(pkg, null, 2)}\n`);
    writeFileSync(join(root, lockFile), `${JSON.stringify(lock, null, 2)}\n`);
  }
  expect(pkgFile, "the version", pkg.version, version);
  expect(lockFile, "the version", lock.version, version);
  expect(lockFile, "the root package's version", lock.packages[""]?.version, version);
  if (linked) expect(lockFile, "the SDK's version", linked.version, version);
}

// --- Version requirements in the docs -------------------------------------

function markdown(dir) {
  return readdirSync(join(root, dir), { withFileTypes: true }).flatMap((entry) => {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) return markdown(path);
    return entry.name.endsWith(".md") ? [path] : [];
  });
}
const crateReadmes = readdirSync(join(root, "crates"))
  .map((name) => `crates/${name}/README.md`)
  .filter((file) => existsSync(join(root, file)));
const requirementLine = /^(proteus-[a-z-]+) = "([^"]+)"$/gm;
for (const file of [...markdown("docs"), ...crateReadmes]) {
  let text = readFileSync(join(root, file), "utf8");
  if (command === "set") {
    text = text.replace(requirementLine, (all, name) => `${name} = "${requirement}"`);
    writeFileSync(join(root, file), text);
  }
  for (const [, name, found] of text.matchAll(requirementLine)) {
    expect(file, `the requirement on ${name}`, found, requirement);
  }
}

// --- Cargo.lock and the changelog -------------------------------------------

if (command === "set") {
  // Records the new version of the workspace's crates; leaves every other
  // dependency where it is.
  execFileSync("cargo", ["update", "--workspace"], { cwd: root, stdio: "inherit" });
  execFileSync("git-cliff", ["--tag", `v${version}`, "--output", "CHANGELOG.md"], { cwd: root, stdio: "inherit" });
}
const lockfile = readFileSync(join(root, "Cargo.lock"), "utf8");
for (const [, name] of internalCrates) {
  const locked = lockfile.match(new RegExp(`^name = "${name}"\nversion = "([^"]+)"$`, "m"))?.[1];
  expect("Cargo.lock", `${name}'s version`, locked, version);
}
const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");
if (!changelog.includes(`\n## ${version} (`)) {
  problems.push(`CHANGELOG.md: no section for ${version}`);
}

if (problems.length > 0) {
  console.error(`version ${version}:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(command === "set" ? `version set to ${version}` : `version ${version} everywhere`);
