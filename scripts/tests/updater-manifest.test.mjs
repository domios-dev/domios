// The v1.9.6 failure shape: a re-run rebuilds an artifact, the merge reads its
// own replaced fragment back through a cache, and the manifest ships the
// previous run's signature over the new bytes. Every client then refuses it.

import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign as edSign } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const MERGE = path.join(ROOT, "scripts", "merge-latest-json.js");
const VERIFY = path.join(ROOT, "scripts", "verify-updater-manifest.mjs");

function scratch() {
  return mkdtempSync(path.join(tmpdir(), "updater-manifest-"));
}

function fragment(dir, name, platform, signature, version = "9.9.9") {
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    path.join(dir, name),
    JSON.stringify({
      version,
      notes: "n",
      pub_date: "2026-01-01T00:00:00Z",
      platforms: { [platform]: { url: `https://example.invalid/${platform}`, signature } },
    }),
  );
}

function merge(args) {
  return execFileSync(process.execPath, [MERGE, ...args], { cwd: ROOT, encoding: "utf8" });
}

test("overlay outranks both the round-tripped fragment and the base manifest", () => {
  const dir = scratch();
  try {
    // What came back from the release assets: the stale run-1 signature.
    fragment(path.join(dir, "in", "frag-0"), "latest.json", "darwin-aarch64", "STALE-FRAGMENT");
    writeFileSync(
      path.join(dir, "base.json"),
      JSON.stringify({
        version: "9.9.9",
        platforms: {
          "darwin-aarch64": { url: "https://example.invalid/old", signature: "STALE-BASE" },
          "windows-x86_64": { url: "https://example.invalid/win", signature: "OTHER-LANE" },
        },
      }),
    );
    // What this run actually signed.
    fragment(path.join(dir, "overlay"), "updater-darwin-aarch64.json", "darwin-aarch64", "FRESH");

    const log = merge([
      "--in", path.join(dir, "in"),
      "--out", path.join(dir, "out.json"),
      "--base", path.join(dir, "base.json"),
      "--overlay", path.join(dir, "overlay"),
    ]);
    const merged = JSON.parse(readFileSync(path.join(dir, "out.json"), "utf8"));

    assert.equal(merged.platforms["darwin-aarch64"].signature, "FRESH");
    // A platform this lane does not own still survives from the base.
    assert.equal(merged.platforms["windows-x86_64"].signature, "OTHER-LANE");
    assert.match(log, /Overlay replaced a stale darwin-aarch64 signature/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("overlay refuses a fragment from a different version", () => {
  const dir = scratch();
  try {
    fragment(path.join(dir, "in", "frag-0"), "latest.json", "darwin-aarch64", "A", "9.9.9");
    fragment(path.join(dir, "overlay"), "updater-darwin-aarch64.json", "darwin-aarch64", "B", "1.0.0");
    assert.throws(
      () =>
        merge([
          "--in", path.join(dir, "in"),
          "--out", path.join(dir, "out.json"),
          "--overlay", path.join(dir, "overlay"),
        ]),
      /is v1\.0\.0, fragments are v9\.9\.9/,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// A minisign keypair + prehashed signature, so the verifier is exercised against
// real Ed25519/BLAKE2b bytes rather than a stub.
function minisignKeypair() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(12);
  const keyId = Buffer.from("0123456789abcdef", "hex");
  const pubkeyField = Buffer.from(
    `untrusted comment: minisign public key\n${Buffer.concat([
      Buffer.from("Ed"),
      keyId,
      raw,
    ]).toString("base64")}\n`,
  ).toString("base64");
  const signatureFor = (content) => {
    const digest = createHash("blake2b512").update(content).digest();
    const signature = edSign(null, digest, privateKey);
    const payload = Buffer.concat([Buffer.from("ED"), keyId, signature]).toString("base64");
    return Buffer.from(
      `untrusted comment: signature from test key\n${payload}\ntrusted comment: test\nAAAA\n`,
    ).toString("base64");
  };
  return { pubkeyField, signatureFor };
}

function verify(dir, manifest, pubkey, extraArgs = []) {
  try {
    const stdout = execFileSync(
      process.execPath,
      [VERIFY, "--manifest", manifest, "--pubkey", pubkey, "--local", dir, ...extraArgs],
      { cwd: ROOT, encoding: "utf8" },
    );
    return { code: 0, stdout };
  } catch (error) {
    return { code: error.status ?? 1, stdout: `${error.stdout ?? ""}${error.stderr ?? ""}` };
  }
}

test("verification passes for signed bytes and fails once the artifact is rebuilt", () => {
  const dir = scratch();
  try {
    const { pubkeyField, signatureFor } = minisignKeypair();
    const artifact = path.join(dir, "CodeTerm_aarch64.app.tar.gz");
    writeFileSync(artifact, Buffer.from("run-1 artifact bytes"));
    const manifest = path.join(dir, "latest.json");
    writeFileSync(
      manifest,
      JSON.stringify({
        version: "9.9.9",
        pub_date: "2026-01-01T00:00:00Z",
        platforms: {
          "darwin-aarch64": {
            url: "https://example.invalid/v9.9.9/CodeTerm_aarch64.app.tar.gz",
            signature: signatureFor(readFileSync(artifact)),
          },
        },
      }),
    );

    const before = verify(dir, manifest, pubkeyField);
    assert.equal(before.code, 0, before.stdout);
    assert.match(before.stdout, /all 1 platform signature\(s\) verify/);

    // The re-run rebuilds the artifact; the manifest keeps the old signature.
    writeFileSync(artifact, Buffer.from("run-2 artifact bytes, rebuilt"));
    const after = verify(dir, manifest, pubkeyField);
    assert.equal(after.code, 1);
    assert.match(after.stdout, /signature does not match the served bytes/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("mac updater contract requires both architectures and maps M2 to the arm artifact", () => {
  const dir = scratch();
  try {
    const { pubkeyField, signatureFor } = minisignKeypair();
    const armArtifact = path.join(dir, "CodeTerm_aarch64.app.tar.gz");
    const intelArtifact = path.join(dir, "CodeTerm_x64.app.tar.gz");
    writeFileSync(armArtifact, Buffer.from("arm64 updater bytes"));
    writeFileSync(intelArtifact, Buffer.from("x64 updater bytes"));
    const manifest = path.join(dir, "latest.json");
    writeFileSync(
      manifest,
      JSON.stringify({
        version: "9.9.9",
        pub_date: "2026-01-01T00:00:00Z",
        platforms: {
          "darwin-aarch64": {
            url: "https://example.invalid/v9.9.9/CodeTerm_aarch64.app.tar.gz",
            signature: signatureFor(readFileSync(armArtifact)),
          },
          "darwin-x86_64": {
            url: "https://example.invalid/v9.9.9/CodeTerm_x64.app.tar.gz",
            signature: signatureFor(readFileSync(intelArtifact)),
          },
        },
      }),
    );
    const required = [
      "--require-platform", "darwin-aarch64",
      "--require-platform", "darwin-x86_64",
    ];

    const valid = verify(dir, manifest, pubkeyField, required);
    assert.equal(valid.code, 0, valid.stdout);
    assert.match(valid.stdout, /all 2 platform signature\(s\) verify/);

    const parsed = JSON.parse(readFileSync(manifest, "utf8"));
    delete parsed.platforms["darwin-aarch64"];
    writeFileSync(manifest, JSON.stringify(parsed));
    const missingArm = verify(dir, manifest, pubkeyField, required);
    assert.equal(missingArm.code, 1);
    assert.match(missingArm.stdout, /missing required platform 'darwin-aarch64'/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("mac updater contract rejects an Intel artifact under the Apple Silicon key", () => {
  const dir = scratch();
  try {
    const { pubkeyField } = minisignKeypair();
    const manifest = path.join(dir, "latest.json");
    writeFileSync(
      manifest,
      JSON.stringify({
        version: "9.9.9",
        platforms: {
          "darwin-aarch64": {
            url: "https://example.invalid/v9.9.9/CodeTerm_x64.app.tar.gz",
            signature: "unused",
          },
        },
      }),
    );

    const result = verify(dir, manifest, pubkeyField);
    assert.equal(result.code, 1);
    assert.match(result.stdout, /darwin-aarch64 must reference CodeTerm_aarch64\.app\.tar\.gz/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
