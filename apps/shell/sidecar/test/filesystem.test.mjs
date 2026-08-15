import assert from "node:assert/strict";
import { mkdtemp, mkdir, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { listProjectFiles, readProjectFile, searchProject } from "../src/filesystem.mjs";

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "foyer-shell-fs-"));
  await mkdir(join(root, "src"));
  await mkdir(join(root, "node_modules"));
  await mkdir(join(root, ".git"));
  await writeFile(join(root, "src", "main.rs"), "fn main() {\n    explain();\n}\n");
  await writeFile(join(root, "README.md"), "# Example\nA readable project.\n");
  await writeFile(join(root, ".env"), "OPENROUTER_API_KEY=not-readable\n");
  await writeFile(join(root, "node_modules", "dependency.js"), "const hidden = true;\n");
  await writeFile(join(root, ".git", "config"), "credential = hidden\n");
  return root;
}

test("project listing is bounded to useful source files", async () => {
  const root = await fixture();
  const listing = await listProjectFiles(root);
  assert.deepEqual(listing.files, ["README.md", "src/main.rs"]);
  assert.equal(listing.truncated, false);
});

test("project search returns numbered excerpts without entering dependencies", async () => {
  const root = await fixture();
  const result = await searchProject(root, "explain");
  assert.deepEqual(result.matches, [{ path: "src/main.rs", line: 2, text: "explain();" }]);
  assert.equal(result.matches.some((match) => match.path.includes("node_modules")), false);
});

test("project reads use bounded numbered ranges and reject secrets", async () => {
  const root = await fixture();
  const result = await readProjectFile(root, "src/main.rs", { startLine: 2, endLine: 2 });
  assert.equal(result.content, "2:     explain();");
  await assert.rejects(() => readProjectFile(root, ".env"), /not readable/);
});

test("project reads reject lexical and symlink escapes", async () => {
  const root = await fixture();
  const outside = await mkdtemp(join(tmpdir(), "foyer-shell-outside-"));
  await writeFile(join(outside, "outside.rs"), "const PRIVATE: bool = true;\n");
  await symlink(join(outside, "outside.rs"), join(root, "src", "linked.rs"));
  await assert.rejects(() => readProjectFile(root, "../outside.rs"), /escapes/);
  await assert.rejects(() => readProjectFile(root, "src/linked.rs"), /escapes/);
});
