#!/usr/bin/env node
// Type-checks every TypeScript snippet in docs/: each ```ts block becomes a
// file in crates/proteus-sdk-web/ts/.snippets/, after the prelude in
// ts-snippet-prelude.ts, and tsc checks them against the SDK's source with
// tsconfig.snippets.json. Run it with `npm run check-snippets` in
// crates/proteus-sdk-web/ts, after `make build-sdk-web`.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const docs = join(root, "docs");
const ts = join(root, "crates/proteus-sdk-web/ts");
const out = join(ts, ".snippets");
const prelude = readFileSync(join(root, "scripts/ts-snippet-prelude.ts"), "utf8");

function pages(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return pages(path);
    return entry.name.endsWith(".md") ? [path] : [];
  });
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

let count = 0;
for (const page of pages(docs)) {
  const name = relative(docs, page).replace(/\.md$/, "").replace(/[\\/]/g, "-");
  const blocks = readFileSync(page, "utf8").matchAll(/^```(?:ts|typescript)\n([\s\S]*?)^```$/gm);
  let n = 0;
  for (const [, code] of blocks) {
    n += 1;
    count += 1;
    // `export {}` makes each file a module, so snippets don't share a scope
    // and can use top-level `await`.
    writeFileSync(join(out, `${name}-${n}.ts`), `${prelude}\n${code}\nexport {};\n`);
  }
}

if (count === 0) {
  console.log("check-ts-snippets: no TypeScript snippets in docs/");
  process.exit(0);
}
try {
  execFileSync("npx", ["tsc", "-p", "tsconfig.snippets.json"], { cwd: ts, stdio: "inherit" });
} catch {
  console.error("check-ts-snippets: a snippet failed; the file names give its page and block number");
  process.exit(1);
}
console.log(`check-ts-snippets: ${count} ${count === 1 ? "snippet" : "snippets"} ok`);
