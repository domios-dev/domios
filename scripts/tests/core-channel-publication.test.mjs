import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import zlib from "node:zlib";
import crypto from "node:crypto";
import { CORE_CHANNEL_ARCHIVE, checksumLine, packageChannel, publicationDecision } from "../publish-core-channel.mjs";
import { runtimeFiles, archiveRuntimeFiles } from "../plugin-runtime-files.mjs";

function tarNames(archive) {
  const bytes = zlib.gunzipSync(fs.readFileSync(archive));
  const names = [];
  for (let offset = 0; offset + 512 <= bytes.length && bytes[offset] !== 0; ) {
    const field = (start, length) => bytes.subarray(offset + start, offset + start + length).toString().replace(/\0.*$/s, "");
    const size = parseInt(field(124, 12).trim() || "0", 8);
    if (field(156, 1) !== "g") names.push(field(0, 100));
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return names;
}

test("only newer master history replaces the public core channel", () => {
  const history = new Map([["old", ["new"]]]);
  const isAncestor = (older, newer) => history.get(older)?.includes(newer) ?? false;
  assert.equal(publicationDecision(null, "new", isAncestor), "publish");
  assert.equal(publicationDecision("old", "new", isAncestor), "publish");
  assert.equal(publicationDecision("new", "new", isAncestor), "skip");
  assert.equal(publicationDecision("new", "old", isAncestor), "skip");
  assert.throws(() => publicationDecision("forked", "new", isAncestor), /does not share history/);
});

test("the channel archive holds plugins/ at its root with a sha256 sidecar", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-core-channel-test-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", ["-C", root, ...args]).toString().trim();
  git("init", "-q");
  fs.mkdirSync(path.join(root, "plugins/provider-demo"), { recursive: true });
  fs.writeFileSync(path.join(root, "plugins/channel.json"), JSON.stringify({ name: "codeterm-core", plugins: [] }));
  fs.writeFileSync(path.join(root, "plugins/provider-demo/plugin.json"), JSON.stringify({ id: "demo" }));
  const pluginRoot = path.join(root, "plugins/provider-demo");
  for (const [name, content] of Object.entries({
    "runtime-files.txt": "plugin.json\nplugin.js\nruntime-files.txt\nui/\nassets/\nREADME.md\n",
    "plugin.js": "runtime", "ui/app.js": "ui", "assets/data.json": "{}", "README.md": "Guide",
    "tests/parser.test.js": "test", "fixtures/log.json": "fixture", "src/plugin.ts": "source",
    "ui/src/main.tsx": "source", "ui/app.test.js": "test", "ui/app.js.map": "source map",
    "package.json": "{}", "AGENTS.md": "procedure", "build.mjs": "build",
  })) {
    fs.mkdirSync(path.dirname(path.join(pluginRoot, name)), { recursive: true });
    fs.writeFileSync(path.join(pluginRoot, name), content);
  }
  git("add", "-A");
  git("-c", "user.name=test", "-c", "user.email=test@example.invalid", "commit", "-q", "-m", "plugins");
  const sha = git("rev-parse", "HEAD");
  const out = path.join(root, "out");
  fs.mkdirSync(out);

  const [archive, checksum, source] = packageChannel(sha, out, (command, args) => execFileSync(command, ["-C", root, ...args]));

  const listed = tarNames(archive);
  assert.ok(listed.includes("channel.json"), listed.join(","));
  assert.ok(listed.includes("provider-demo/plugin.json"), listed.join(","));
  const expected = ["README.md", "assets/data.json", "plugin.js", "plugin.json", "runtime-files.txt", "ui/app.js"];
  assert.deepEqual(runtimeFiles(pluginRoot), expected);
  const hash = crypto.createHash("sha256");
  for (const name of expected) {
    hash.update(crypto.createHash("sha256").update(name).update(Buffer.from([0])).update(fs.readFileSync(path.join(pluginRoot, name))).digest());
  }
  assert.equal(hash.digest("hex"), "73dfbc1b9a7318aee3b291a2c1aef82d12f9ab68b460f5b9eb094fc51542a985");
  assert.deepEqual(listed.filter(name => !name.endsWith("/")).sort(), ["channel.json", ...expected.map(name => `provider-demo/${name}`)].sort());
  const individual = path.join(out, "demo.tar.gz");
  archiveRuntimeFiles(sha, "plugins/provider-demo", individual, (command, args) => execFileSync(command, ["-C", root, ...args]));
  assert.deepEqual(tarNames(individual).filter(name => !name.endsWith("/")).sort(), expected);
  assert.equal(fs.readFileSync(checksum, "utf8"), checksumLine(fs.readFileSync(archive), CORE_CHANNEL_ARCHIVE));
  assert.equal(fs.readFileSync(source, "utf8"), `${sha}\n`);
});

test("official channel runtime lists retain manifest assets and exclude development material", () => {
  const pluginsRoot = path.resolve(import.meta.dirname, "../../plugins");
  const channel = JSON.parse(fs.readFileSync(path.join(pluginsRoot, "channel.json"), "utf8"));
  for (const plugin of channel.plugins) {
    const files = runtimeFiles(path.join(pluginsRoot, plugin.path));
    assert.ok(files.includes("plugin.js"), plugin.id);
    assert.ok(files.includes("plugin.json"), plugin.id);
    assert.ok(files.every(name => !/^(tests|fixtures|src|references)\//.test(name) && !["package.json", "CANONICAL-ORACLES.md", "SIDE-CHANNELS.md"].includes(name)), plugin.id);
  }
});
