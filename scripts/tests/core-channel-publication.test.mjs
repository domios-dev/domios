import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import zlib from "node:zlib";
import { CORE_CHANNEL_ARCHIVE, checksumLine, packageChannel, publicationDecision } from "../publish-core-channel.mjs";

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
  git("add", "-A");
  git("-c", "user.name=test", "-c", "user.email=test@example.invalid", "commit", "-q", "-m", "plugins");
  const sha = git("rev-parse", "HEAD");
  const out = path.join(root, "out");
  fs.mkdirSync(out);

  const [archive, checksum, source] = packageChannel(sha, out, (command, args) => execFileSync(command, ["-C", root, ...args]));

  const listed = tarNames(archive);
  assert.ok(listed.includes("channel.json"), listed.join(","));
  assert.ok(listed.includes("provider-demo/plugin.json"), listed.join(","));
  assert.equal(fs.readFileSync(checksum, "utf8"), checksumLine(fs.readFileSync(archive), CORE_CHANNEL_ARCHIVE));
  assert.equal(fs.readFileSync(source, "utf8"), `${sha}\n`);
});
