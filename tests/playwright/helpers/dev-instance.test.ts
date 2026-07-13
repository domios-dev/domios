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
  playwrightCargoTargetDir,
  teardownInstances,
  type DevInstance,
  type InstanceLifecycle,
} from "./dev-instance";

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
    assert.equal(playwrightCargoTargetDir(instance, "win32"), undefined);
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
    assert.deepEqual(devStartArgs(), [
      "build.sh",
      "dev",
      "--wait",
      "--release-gateway-after-ready",
    ]);
    assert.equal(playwrightCargoTargetDir(first, "win32"), "src-tauri/target/playwright-pw1");
    assert.equal(playwrightCargoTargetDir(second, "win32"), "src-tauri/target/playwright-pw2");
    assert.equal(playwrightCargoTargetDir(first, "linux"), undefined);
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
    const env = buildDevInstanceEnv(first, BASE_ENV, "win32", 2);
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

test("two slots route to disjoint env with private cargo target dirs on Windows", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const one = buildDevInstanceEnv(devInstanceForSlot(0, 2), BASE_ENV, "win32", 2);
    const two = buildDevInstanceEnv(devInstanceForSlot(1, 2), BASE_ENV, "win32", 2);
    assert.notEqual(one.CODETERM_INSTANCE, two.CODETERM_INSTANCE);
    assert.notEqual(one.CODETERM_API_PORT, two.CODETERM_API_PORT);
    assert.notEqual(one.CODETERM_CDP_PORT, two.CODETERM_CDP_PORT);
    assert.notEqual(one.CT_E2E_DATADIR, two.CT_E2E_DATADIR);
    assert.equal(one.CARGO_TARGET_DIR, "src-tauri/target/playwright-pw1");
    assert.equal(two.CARGO_TARGET_DIR, "src-tauri/target/playwright-pw2");
    assert.notEqual(one.CARGO_TARGET_DIR, two.CARGO_TARGET_DIR);
  });
});

test("serial and non-Windows runs leave the base cargo target dir untouched", () => {
  withEnv({}, () => {
    const serial = buildDevInstanceEnv(devInstanceForSlot(0), BASE_ENV, "win32", 1);
    assert.equal(serial.CARGO_TARGET_DIR, "/from-base");
    const linux = buildDevInstanceEnv(devInstanceForSlot(0, 2), BASE_ENV, "linux", 2);
    assert.equal(linux.CARGO_TARGET_DIR, "/from-base");
  });
});

test("building env never mutates the caller's environment", () => {
  const base = { ...BASE_ENV };
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    buildDevInstanceEnv(devInstanceForSlot(0, 2), base, "win32", 2);
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
