/**
 * WHAT: the stub gate decides from the daemon's declared `stub_replay` and the
 *      local replayer's `--health` receipt.
 * WHY: a spawn probe read a slow launch as "no stub" and launched the real
 *      provider when the stub was absent.
 * ORACLE: the gate verdict, the health-call count, and zero spawn calls.
 */
import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import type { ApiClient } from "./api-client";
import { repoRoot } from "./dev-instance";
import {
  localReplayPath,
  parseHealthReceipt,
  resolveStubTransport,
  runReplayHealth,
  type StubHealthReceipt,
} from "./stub-gate";

const REPLAY_REL = "tests/playwright/fixtures/stub-agent/replay.mjs";

function fakeApi(providers: unknown[]): { api: Pick<ApiClient, "get">; posts: string[] } {
  const posts: string[] = [];
  const api = {
    async get<T>(p: string): Promise<T> {
      if (p === "/agents/providers") return providers as T;
      throw new Error(`unexpected GET ${p}`);
    },
    async post<T>(p: string): Promise<T> {
      posts.push(p);
      throw new Error(`gate must not POST ${p}`);
    },
  };
  return { api, posts };
}

function healthStub(receipt: StubHealthReceipt) {
  const calls: string[] = [];
  return {
    calls,
    runHealth: async (replayPath: string) => {
      calls.push(replayPath);
      return receipt;
    },
  };
}

const localCommand = `node ${REPLAY_REL}`;

test("stubbed and healthy local replayer is active without any spawn", async () => {
  const { api, posts } = fakeApi([{ id: "claude", stub_replay: true }]);
  const health = healthStub({ ok: true, providers: ["claude"] });
  const active = await resolveStubTransport(api, {
    stubCommand: localCommand,
    root: repoRoot(),
    runHealth: health.runHealth,
  });
  assert.equal(active, true);
  assert.deepEqual(health.calls, [path.resolve(repoRoot(), REPLAY_REL)]);
  assert.deepEqual(posts, []);
});

test("unstubbed or release daemon is not configured and runs no health check", async () => {
  for (const listing of [
    [{ id: "claude", stub_replay: false }, { id: "codex", stub_replay: true }],
    [{ id: "claude" }],
    [],
  ]) {
    const { api, posts } = fakeApi(listing);
    const health = healthStub({ ok: true });
    const active = await resolveStubTransport(api, {
      stubCommand: localCommand,
      root: repoRoot(),
      runHealth: health.runHealth,
    });
    assert.equal(active, false, JSON.stringify(listing));
    assert.deepEqual(health.calls, []);
    assert.deepEqual(posts, []);
  }
});

test("a failed health receipt throws with its reason", async () => {
  const { api, posts } = fakeApi([{ id: "claude", stub_replay: true }]);
  const health = healthStub({ ok: false, reason: "cast for codex unreadable at /x/raw.cast: ENOENT" });
  await assert.rejects(
    resolveStubTransport(api, { stubCommand: localCommand, root: repoRoot(), runHealth: health.runHealth }),
    /cast for codex unreadable at \/x\/raw\.cast: ENOENT/,
  );
  assert.deepEqual(posts, []);
});

test("a stub command without a local replayer trusts the declared daemon state", async () => {
  const { api } = fakeApi([{ id: "claude", stub_replay: true }]);
  const health = healthStub({ ok: false, reason: "must not run" });
  for (const stubCommand of [undefined, "node /nowhere/replay.mjs", "my-stub --flag"]) {
    assert.equal(
      await resolveStubTransport(api, { stubCommand, root: repoRoot(), runHealth: health.runHealth }),
      true,
    );
  }
  assert.deepEqual(health.calls, []);
});

test("localReplayPath unquotes and resolves the replayer token", () => {
  const absolute = path.resolve(repoRoot(), REPLAY_REL);
  assert.equal(localReplayPath(`node '${absolute}' --stub-provider claude`, repoRoot()), absolute);
  assert.equal(localReplayPath(undefined, repoRoot()), null);
});

test("parseHealthReceipt keeps the receipt and reports an unparseable one", () => {
  assert.deepEqual(parseHealthReceipt('{"ok":false,"reason":"r"}\n', "exit 1"), { ok: false, reason: "r" });
  assert.deepEqual(parseHealthReceipt("garbage", "exit 1"), { ok: false, reason: "exit 1" });
});

test("runReplayHealth reads the real replayer receipt", async () => {
  const receipt = await runReplayHealth(path.resolve(repoRoot(), REPLAY_REL));
  assert.equal(receipt.ok, true, receipt.reason);
});
