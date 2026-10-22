import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { relatedSelection } from "../affected-tests.mjs";
import { executeRelatedSelection } from "../test-related.mjs";

test("combined selection owns Rust modules, crate roots and TS importers", t => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "test-related-"));
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }));
  const write = (file, body = "") => {
    fs.mkdirSync(path.dirname(path.join(repo, file)), { recursive: true });
    fs.writeFileSync(path.join(repo, file), body);
  };
  write("src-tauri/src/lib.rs"); write("src-tauri/src/alpha.rs"); write("src-tauri/Cargo.toml");
  write("src/model.ts", "export const opaque = 1");
  write("src/consumer.test.ts", 'import { opaque } from "./model";');
  write("src/unrelated.test.ts", "export {};");
  write("scripts/vitest-owned-tests.json", "[]");
  fs.mkdirSync(path.join(repo, "packages"));
  const metadata = path.join(repo, "metadata.json");
  fs.writeFileSync(metadata, JSON.stringify({ packages: [{ name: "codeterm", manifest_path: path.join(repo, "src-tauri/Cargo.toml"),
    targets: [{ kind: ["lib"], src_path: path.join(repo, "src-tauri/src/lib.rs") }] }] }));
  const options = { repo, metadata };
  assert.deepEqual(relatedSelection(["src-tauri/src/alpha.rs", "src/model.ts"], options), [
    ["./build.sh", "nextest", "shell", "test(/^alpha::/)"],
    ["pnpm", "exec", "vitest", "related", "--run", "--maxWorkers=2", "--passWithNoTests", "src/model.ts"],
    ["pnpm", "test:unit", "--", "--related", "src/model.ts"],
  ]);
  assert.deepEqual(relatedSelection(["src-tauri/src/lib.rs"], options), [["./build.sh", "nextest", "shell", "all()"]]);
  assert.throws(() => relatedSelection([], options), /requires/);
  assert.throws(() => relatedSelection(["../foreign.rs"], options), /outside/);
  assert.throws(() => relatedSelection(["src/missing.ts"], options), /does not exist/);
});

test("prints the combined selection before running exact argv and preserves a failure", () => {
  const commands = [["./build.sh", "nextest", "shell", "test(/^opaque::/)"], ["pnpm", "exec", "vitest", "related", "src/space name.ts"]];
  const events = [];
  const status = executeRelatedSelection(commands, { shell: "owned-bash", write: value => events.push(value), run: (shell, args) => {
    assert.equal(events.length >= 2, true);
    assert.equal(shell, "owned-bash");
    events.push(args);
    return { status: args.includes("pnpm") ? 43 : 0 };
  } });
  assert.equal(status, 43);
  assert.deepEqual(events[2].slice(3), commands[0]);
  assert.deepEqual(events[3].slice(3), commands[1]);
  let calls = 0;
  assert.equal(executeRelatedSelection(commands, { write() {}, run() { calls++; return { status: 75 }; } }), 75);
  assert.equal(calls, 1);
});
