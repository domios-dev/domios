/**
 * WHAT: Playwright worker slots resolve to isolated CodeTerm dev instances.
 * WHY: Parallel workers must never share API, UI, token, or mutable state.
 * ORACLE: Each slot has the expected pwN identity and disjoint ports/data dirs.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
  buildDevInstanceEnv,
  currentDevInstance,
  devInstanceForSlot,
  devInstanceTeardownPorts,
  devStartArgs,
  normalizeStubAgentCmd,
  playwrightCargoTargetDir,
  teardownInstances,
  type DevInstance,
  type InstanceLifecycle,
} from "./dev-instance";

function hasRelativeReplay(cmd: string): boolean {
  const raw = /(?:^|\s)('[^']*replay\.mjs'|"[^"]*replay\.mjs"|[^\s'"][^\s]*replay\.mjs|replay\.mjs)(?:\s|$)/.exec(cmd)?.[1];
  if (!raw) return false;
  const q = raw[0];
  const token = (q === "'" || q === '"') && raw.endsWith(q) ? raw.slice(1, -1) : raw;
  return !(token.startsWith("/") || /^[A-Za-z]:[\\/]/.test(token) || token.startsWith("\\\\"));
}

const ENV_KEYS = [
  "CT_E2E_WORKERS",
  "CT_E2E_PORT",
  "CT_E2E_PORT_BASE",
  "CT_E2E_INSTANCE",
  "CT_E2E_DATADIR",
  "TEST_PARALLEL_INDEX",
] as const;

type EnvValues = Partial<Record<(typeof ENV_KEYS)[number], string>>;

function applyEnv(values: EnvValues): Record<string, string | undefined> {
  const saved = Object.fromEntries(ENV_KEYS.map((key) => [key, process.env[key]]));
  for (const key of ENV_KEYS) delete process.env[key];
  Object.assign(process.env, values);
  return saved;
}

function restoreEnv(saved: Record<string, string | undefined>): void {
  for (const key of ENV_KEYS) {
    const value = saved[key];
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
}

function withEnv(values: EnvValues, run: () => void): void {
  const saved = applyEnv(values);
  try {
    run();
  } finally {
    restoreEnv(saved);
  }
}

async function withEnvAsync(values: EnvValues, run: () => Promise<void>): Promise<void> {
  const saved = applyEnv(values);
  try {
    await run();
  } finally {
    restoreEnv(saved);
  }
}

test("serial defaults use the isolated Playwright instance", () => {
  withEnv({}, () => {
    const instance = devInstanceForSlot(0);
    assert.equal(instance.name, "playwright");
    assert.equal(instance.apiPort, 7786);
    assert.equal(instance.dataDirName, ".codeterm-dev-playwright");
    assert.deepEqual(devStartArgs(), ["build.sh", "dev", "--wait"]);
    assert.equal(playwrightCargoTargetDir(instance, {}), undefined);
  });
});

test("an explicit serial instance receives isolated UI ports", () => {
  withEnv({ CT_E2E_INSTANCE: "pwserial", CT_E2E_PORT: "7790" }, () => {
    const instance = devInstanceForSlot(0);
    assert.equal(instance.name, "pwserial");
    assert.equal(instance.apiPort, 7790);
    assert.notEqual(instance.vitePort, 1420);
    assert.notEqual(instance.cdpPort, 9223);
  });
});

test("two worker slots have disjoint named instances", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const first = devInstanceForSlot(0);
    const second = devInstanceForSlot(1);
    assert.deepEqual(
      [first.name, second.name],
      ["pw1", "pw2"],
    );
    assert.equal(first.apiPort, 7786);
    assert.equal(second.apiPort, 7787);
    assert.notEqual(first.vitePort, second.vitePort);
    assert.notEqual(first.cdpPort, second.cdpPort);
    assert.notEqual(first.dataDirName, second.dataDirName);
    assert.deepEqual(devStartArgs(), ["build.sh", "dev", "--wait"]);
    assert.equal(playwrightCargoTargetDir(first, {}), undefined);
    assert.equal(playwrightCargoTargetDir(second, {}), undefined);
  });
});

test("parallelIndex pins the worker process to its instance slot", () => {
  withEnv({ CT_E2E_WORKERS: "2", TEST_PARALLEL_INDEX: "1" }, () => {
    const instance = currentDevInstance();
    assert.equal(instance.slot, 1);
    assert.equal(instance.name, "pw2");
  });
});

test("prod and out-of-range slots are rejected", () => {
  withEnv({ CT_E2E_PORT: "7685" }, () => {
    assert.throws(() => devInstanceForSlot(0), /unsafe Playwright instance port 7685/);
  });
  withEnv({ CT_E2E_PORT: "7686" }, () => {
    assert.throws(() => devInstanceForSlot(0), /unsafe Playwright instance port 7686/);
  });
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    assert.throws(() => devInstanceForSlot(2), /outside 0\.\.1/);
  });
});

const BASE_ENV = { PATH: "/bin", CARGO_TARGET_DIR: "/from-base" } as const;

test("build env pins each slot to its own instance, ports, and data dir", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const first = devInstanceForSlot(0, 2);
    const env = buildDevInstanceEnv(first, BASE_ENV);
    assert.equal(env.CODETERM_INSTANCE, "pw1");
    assert.equal(env.CODETERM_API_PORT, String(first.apiPort));
    assert.equal(env.CODETERM_VITE_PORT, String(first.vitePort));
    assert.equal(env.CODETERM_CDP_PORT, String(first.cdpPort));
    assert.equal(env.CT_E2E_DATADIR, first.dataDirName);
    assert.equal(env.CT_E2E_PORT, String(first.apiPort));
    assert.equal(env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, `--remote-debugging-port=${first.cdpPort}`);
    assert.equal(env.VITE_CODETERM_TEST, "1");
    assert.equal(env.CODETERM_TEST_CLOCK, "1");
    assert.equal(env.PATH, "/bin");
  });
});

test("two slots leave cargo target ownership to their named build instances", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const one = buildDevInstanceEnv(devInstanceForSlot(0, 2), { PATH: "/bin" });
    const two = buildDevInstanceEnv(devInstanceForSlot(1, 2), { PATH: "/bin" });
    assert.notEqual(one.CODETERM_INSTANCE, two.CODETERM_INSTANCE);
    assert.notEqual(one.CODETERM_API_PORT, two.CODETERM_API_PORT);
    assert.notEqual(one.CODETERM_CDP_PORT, two.CODETERM_CDP_PORT);
    assert.notEqual(one.CT_E2E_DATADIR, two.CT_E2E_DATADIR);
    assert.equal(one.CARGO_TARGET_DIR, undefined);
    assert.equal(two.CARGO_TARGET_DIR, undefined);
    assert.equal("CARGO_TARGET_DIR" in one, false);
    assert.equal("CARGO_TARGET_DIR" in two, false);
  });
});

test("an explicit caller cargo target override is preserved", () => {
  withEnv({}, () => {
    const serial = buildDevInstanceEnv(devInstanceForSlot(0), BASE_ENV);
    assert.equal(serial.CARGO_TARGET_DIR, "/from-base");
    assert.equal(playwrightCargoTargetDir(devInstanceForSlot(0), BASE_ENV), "/from-base");
  });
});

test("building env never mutates the caller's environment", () => {
  const base = { ...BASE_ENV };
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    buildDevInstanceEnv(devInstanceForSlot(0, 2), base);
  });
  assert.deepEqual(base, BASE_ENV);
});

test("teardown ports are exactly the instance listeners and never prod", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const first = devInstanceForSlot(0, 2);
    const second = devInstanceForSlot(1, 2);
    assert.deepEqual(devInstanceTeardownPorts(first), [
      first.apiPort,
      first.vitePort,
      first.cdpPort,
    ]);
    const all = [
      ...devInstanceTeardownPorts(first),
      ...devInstanceTeardownPorts(second),
    ];
    assert.equal(new Set(all).size, all.length, "slot ports must be disjoint");
    assert.ok(!all.includes(7685) && !all.includes(7686), "must never target prod/dev");
  });
});

function recordingLifecycle(failOn: (instance: DevInstance) => boolean): {
  lifecycle: InstanceLifecycle;
  killed: string[];
} {
  const killed: string[] = [];
  const lifecycle: InstanceLifecycle = {
    kill: (instance) => killed.push(instance.name),
    assertStopped: async (instance) => {
      if (failOn(instance)) throw new Error(`leak on ${instance.name}`);
    },
  };
  return { lifecycle, killed };
}

test("teardown kills every instance in reverse start order", async () => {
  await withEnvAsync({ CT_E2E_WORKERS: "2" }, async () => {
    const instances = [devInstanceForSlot(0, 2), devInstanceForSlot(1, 2)];
    const { lifecycle, killed } = recordingLifecycle(() => false);
    await teardownInstances(instances, lifecycle);
    assert.deepEqual(killed, ["pw2", "pw1"]);
  });
});

test("a leaking instance still lets the rest tear down, then surfaces together", async () => {
  await withEnvAsync({ CT_E2E_WORKERS: "2" }, async () => {
    const instances = [devInstanceForSlot(0, 2), devInstanceForSlot(1, 2)];
    const { lifecycle, killed } = recordingLifecycle((i) => i.name === "pw2");
    await assert.rejects(
      () => teardownInstances(instances, lifecycle),
      (error: unknown) => {
        assert.ok(error instanceof AggregateError);
        assert.equal(error.errors.length, 1);
        assert.match(String(error.errors[0]), /leak on pw2/);
        return true;
      },
    );
    assert.deepEqual(killed, ["pw2", "pw1"], "both instances killed despite the leak");
  });
});

test("a synchronous kill failure still tears down the rest and surfaces", async () => {
  await withEnvAsync({ CT_E2E_WORKERS: "2" }, async () => {
    const instances = [devInstanceForSlot(0, 2), devInstanceForSlot(1, 2)];
    const killed: string[] = [];
    const asserted: string[] = [];
    const lifecycle: InstanceLifecycle = {
      kill: (instance) => {
        killed.push(instance.name);
        if (instance.name === "pw2") throw new Error(`kill failed on ${instance.name}`);
      },
      assertStopped: async (instance) => {
        asserted.push(instance.name);
      },
    };
    await assert.rejects(
      () => teardownInstances(instances, lifecycle),
      (error: unknown) => {
        assert.ok(error instanceof AggregateError);
        assert.equal(error.errors.length, 1);
        assert.match(String(error.errors[0]), /kill failed on pw2/);
        return true;
      },
    );
    assert.deepEqual(killed, ["pw2", "pw1"], "both kills attempted despite pw2 throwing");
    assert.deepEqual(asserted, ["pw1"], "pw1 still asserted; pw2 skipped after its kill threw");
  });
});

const REPLAY_REL = "tests/playwright/fixtures/stub-agent/replay.mjs";

test("POSIX relative replay path resolves to absolute under the repo root", () => {
  const root = "/Users/dev/codeterm";
  const cmd = normalizeStubAgentCmd(`node ${REPLAY_REL}`, root);
  assert.equal(cmd, `node ${root}/${REPLAY_REL}`);
  assert.ok(!hasRelativeReplay(cmd!), "no repo-relative replay token remains");
});

test("only the replay token is rewritten; node, flags, and other relative tokens are preserved", () => {
  const root = "/Users/dev/codeterm";
  const cmd = `node ${REPLAY_REL} --stub-provider --model foo -- other/rel.mjs`;
  const out = normalizeStubAgentCmd(cmd, root)!;
  assert.equal(out, `node ${root}/${REPLAY_REL} --stub-provider --model foo -- other/rel.mjs`);
  assert.ok(out.includes(" --stub-provider --model foo -- other/rel.mjs"), "flags + other relative token byte-stable");
});

test("space-containing repo root yields a single-quoted absolute token", () => {
  const root = "/Users/a b/codeterm";
  const cmd = normalizeStubAgentCmd(`node ${REPLAY_REL}`, root)!;
  assert.equal(cmd, `node '${root}/${REPLAY_REL}'`, "space-containing path stays one single-quoted token");
  assert.ok(!cmd.includes('"'), "single quotes, not double");
});

test("a single-quoted relative replay path resolves cleanly under a space-containing root", () => {
  const root = "/Users/a b/codeterm";
  const cmd = normalizeStubAgentCmd(`node '${REPLAY_REL}'`, root)!;
  assert.equal(cmd, `node '${root}/${REPLAY_REL}'`, "input single quotes stripped, clean absolute emitted");
  assert.ok(!/\/'|'tests/.test(cmd), "no embedded input-quote fragments");
  assert.ok(!hasRelativeReplay(cmd), "no repo-relative replay token remains");
  assert.equal(normalizeStubAgentCmd(cmd, root), cmd, "idempotent");
});

test("a double-quoted relative replay path resolves cleanly under a space-containing root", () => {
  const root = "/Users/a b/codeterm";
  const cmd = normalizeStubAgentCmd(`node "${REPLAY_REL}"`, root)!;
  assert.equal(cmd, `node '${root}/${REPLAY_REL}'`, "double input quote stripped, single-quote emitted");
  assert.ok(!/\/"|"tests/.test(cmd), "no embedded input-quote fragments");
  assert.ok(!hasRelativeReplay(cmd), "no repo-relative replay token remains");
  assert.equal(normalizeStubAgentCmd(cmd, root), cmd, "idempotent");
});

test("unbalanced or mixed quoting around the replay token is left byte-stable", () => {
  const root = "/Users/a b/codeterm";
  const inputs = [
    `node "${REPLAY_REL}`,
    `node '${REPLAY_REL}`,
    `node '${REPLAY_REL}"`,
    `node "${REPLAY_REL}'`,
  ];
  for (const cmd of inputs) {
    assert.equal(normalizeStubAgentCmd(cmd, root), cmd, `unbalanced/mixed not repaired: ${cmd}`);
    assert.equal(normalizeStubAgentCmd(cmd, root), normalizeStubAgentCmd(normalizeStubAgentCmd(cmd, root), root), "idempotent");
  }
});

test("already-absolute replay paths are byte-stable across the C:\\, C:/, POSIX /, and UNC forms", () => {
  const matrix = [
    "node C:\\ct\\replay.mjs --flag",
    "node C:/ct/replay.mjs --flag",
    "node /opt/codeterm/replay.mjs",
    "node \\\\host\\ct\\replay.mjs",
  ];
  for (const cmd of matrix) {
    assert.equal(normalizeStubAgentCmd(cmd, "/Users/dev/codeterm"), cmd);
  }
});

test("an unrelated command with no replay.mjs is byte-stable", () => {
  const cmd = "node fixtures/stub-agent/other.mjs --flag value";
  assert.equal(normalizeStubAgentCmd(cmd, "/Users/dev/codeterm"), cmd);
  assert.equal(normalizeStubAgentCmd(undefined, "/Users/dev/codeterm"), undefined);
});

test("idempotency pins the single-quote-if-needed output policy", () => {
  // wantsQuote: the resolved absolute path contains whitespace.
  const cases: [string, string, boolean][] = [
    [`node ${REPLAY_REL}`, "/Users/dev/codeterm", false],
    [`node ${REPLAY_REL}`, "/Users/a b/codeterm", true],
    ["node C:\\ct\\replay.mjs", "/Users/dev/codeterm", false],
    ["node C:/ct/replay.mjs", "/Users/dev/codeterm", false],
    ["node /opt/codeterm/replay.mjs", "/Users/dev/codeterm", false],
    ["node fixtures/other.mjs --flag v", "/Users/dev/codeterm", false],
  ];
  for (const [cmd, root, wantsQuote] of cases) {
    const once = normalizeStubAgentCmd(cmd, root)!;
    assert.ok(!once.includes('"'), `never emits double quotes: ${once}`);
    assert.equal(once.includes("'"), wantsQuote, `single-quote iff whitespace: ${once}`);
    assert.equal(normalizeStubAgentCmd(once, root), once, `second pass byte-stable, same quote form: ${once}`);
  }
});

test("normalizing the stub cmd never mutates the caller's base env", () => {
  const base = { PATH: "/bin", CODETERM_STUB_AGENT_CMD: `node ${REPLAY_REL}` } as const;
  const snapshot = { ...base };
  buildDevInstanceEnv(devInstanceForSlot(0), base);
  assert.deepEqual(base, snapshot, "baseEnv untouched");
});

test("the resulting daemon env carries no repo-relative replay token", () => {
  const base = { PATH: "/bin", CODETERM_STUB_AGENT_CMD: `node ${REPLAY_REL}` } as const;
  const env = buildDevInstanceEnv(devInstanceForSlot(0), base);
  assert.ok(!hasRelativeReplay(env.CODETERM_STUB_AGENT_CMD!), "env replay token is absolute");
});
