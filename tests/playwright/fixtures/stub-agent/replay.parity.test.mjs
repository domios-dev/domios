// Parity gate: the bytes the stub emits for its boot transcript are exactly
// the recorded real-session bytes, so detection/parsing downstream of the PTY
// pipeline see byte-identical input. Run: node --test replay.parity.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DEFAULT_CASTS, castOutput } from "./replay-cast.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

// The replayer truncates at CODETERM_STUB_REPLAY_BYTE_CAP; lift it past the
// largest default cast so the gate compares whole transcripts, not prefixes.
const FULL_TRANSCRIPT_CAP = 8_000_000;

function castBytes(castPath) {
  return castOutput(readFileSync(path.join(REPO, castPath), "utf8"), FULL_TRANSCRIPT_CAP);
}

// An ambient CODETERM_STUB_CAST would silently replace the map under test.
function replayEnv(extra = {}) {
  const env = { ...process.env, ...extra };
  delete env.CODETERM_STUB_CAST;
  return env;
}

function encodeClaudeCwd(cwd) {
  return Array.from(String(cwd || ""))
    .map((c) => (/[\p{L}\p{N}]/u.test(c) ? c : "-"))
    .join("");
}

for (const [provider, castPath] of Object.entries(DEFAULT_CASTS)) {
  test(`${provider}: stub boot replays the recorded real transcript byte-for-byte`, () => {
    const stdout = execFileSync(
      process.execPath,
      [path.join(HERE, "replay.mjs"), "--stub-provider", provider, "--", provider, "--model", "x"],
      {
        input: "",
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
        env: replayEnv({ CODETERM_STUB_REPLAY_BYTE_CAP: String(FULL_TRANSCRIPT_CAP) }),
      },
    );
    const recorded = castBytes(castPath);
    assert.ok(recorded.length > 1000, "recorded cast is non-trivial");
    assert.ok(
      stdout.includes(recorded),
      `${provider} replay output must embed the recorded transcript verbatim`,
    );
  });
}

test("claude: scripted turns persist under the native provider project dir", () => {
  const cwd = mkdtempSync(path.join(os.tmpdir(), "pw_stub_agent_under_score_"));
  const nonce = `PARITY-${Date.now()}`;
  try {
    execFileSync(
      process.execPath,
      [
        path.join(HERE, "replay.mjs"),
        "--stub-provider",
        "claude",
        "--",
        "claude",
        "--model",
        "x",
        "--",
        `STUB_ECHO ${nonce}`,
      ],
      { cwd, input: "", encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
    );
    const encoded = encodeClaudeCwd(realpathSync(cwd));
    const dir = path.join(os.homedir(), ".claude", "projects", encoded);
    assert.ok(existsSync(dir), `expected native provider dir ${dir}`);
    const sessionFiles = new Set(
      existsSync(dir) ? readdirSync(dir).filter((name) => name.endsWith(".jsonl")) : [],
    );
    assert.ok(sessionFiles.size > 0, `expected a session jsonl in ${dir}`);
    const hit = Array.from(sessionFiles).some((file) =>
      readFileSync(path.join(dir, file), "utf8").includes(`ack ${nonce}`),
    );
    assert.ok(hit, "scripted assistant reply must be readable from the native provider path");
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

test("claude: scripted turn records retain the session-tailer shape", () => {
  const cwd = mkdtempSync(path.join(os.tmpdir(), "pw_stub_agent_record_shape_"));
  const sessionId = `fixture-shape-${randomUUID()}`;
  const encoded = encodeClaudeCwd(realpathSync(cwd));
  const sessionFile = path.join(os.homedir(), ".claude", "projects", encoded, `${sessionId}.jsonl`);
  try {
    execFileSync(
      process.execPath,
      [
        path.join(HERE, "replay.mjs"),
        "--stub-provider",
        "claude",
        "--",
        "claude",
        "--model",
        "x",
        "--session-id",
        sessionId,
        "--",
        "STUB_ECHO opaque-marker",
      ],
      { cwd, input: "", encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
    );
    const records = readFileSync(sessionFile, "utf8")
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));
    assert.equal(records.length, 2);

    const [user, assistant] = records;
    assert.equal(user.type, "user");
    assert.equal(user.isSidechain, false);
    assert.equal(user.parentUuid, null);
    assert.equal(user.message.role, "user");
    assert.equal(typeof user.message.content, "string");
    assert.equal(user.sessionId, sessionId);
    assert.equal(user.cwd, cwd);
    assert.match(user.timestamp, /^\d{4}-\d{2}-\d{2}T/);
    assert.match(user.uuid, /^[0-9a-f-]{36}$/i);

    assert.equal(assistant.type, "assistant");
    assert.equal(assistant.isSidechain, false);
    assert.equal(assistant.parentUuid, user.uuid);
    assert.equal(assistant.message.type, "message");
    assert.equal(assistant.message.role, "assistant");
    assert.equal(assistant.message.model, "x");
    assert.equal(assistant.message.content[0].type, "text");
    assert.equal(typeof assistant.message.content[0].text, "string");
    assert.equal(assistant.sessionId, sessionId);
    assert.equal(assistant.cwd, cwd);
    assert.match(assistant.timestamp, /^\d{4}-\d{2}-\d{2}T/);
    assert.match(assistant.uuid, /^[0-9a-f-]{36}$/i);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
    rmSync(sessionFile, { force: true });
  }
});
