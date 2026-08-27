import assert from "node:assert/strict";
import test from "node:test";
import {
  MESH_DAEMON_BASE_ENV,
  assertHarnessDaemonBase,
  resolveHarnessDaemonBase,
} from "./daemon-base";

const SUPPLIED_INSTANCE_BASE = "http://127.0.0.1:7791";
const SUPPLIED_OVERRIDE = "http://127.0.0.1:7893";
const SUPPLIED_EXPLICIT = "http://127.0.0.1:7994";

test("resolves the base the harness instance supplies", () => {
  assert.equal(
    resolveHarnessDaemonBase({ env: {}, instanceBase: () => SUPPLIED_INSTANCE_BASE }),
    SUPPLIED_INSTANCE_BASE,
  );
});

test("the env override beats the instance base", () => {
  assert.equal(
    resolveHarnessDaemonBase({
      env: { [MESH_DAEMON_BASE_ENV]: SUPPLIED_OVERRIDE },
      instanceBase: () => SUPPLIED_INSTANCE_BASE,
    }),
    SUPPLIED_OVERRIDE,
  );
});

test("an explicit base beats both the env override and the instance base", () => {
  assert.equal(
    resolveHarnessDaemonBase({
      explicit: SUPPLIED_EXPLICIT,
      env: { [MESH_DAEMON_BASE_ENV]: SUPPLIED_OVERRIDE },
      instanceBase: () => SUPPLIED_INSTANCE_BASE,
    }),
    SUPPLIED_EXPLICIT,
  );
});

// ── negative controls: the defect is a SILENT default, so every unresolvable
//    input must raise instead of producing any base at all. ──

test("an instance that cannot describe itself raises instead of defaulting", () => {
  assert.throws(
    () =>
      resolveHarnessDaemonBase({
        env: {},
        instanceBase: () => {
          throw new Error("TEST_PARALLEL_INDEX must be an integer, got banana");
        },
      }),
    /unresolvable from the dev instance/,
  );
});

test("an empty instance base raises instead of defaulting", () => {
  assert.throws(
    () => resolveHarnessDaemonBase({ env: {}, instanceBase: () => "" }),
    /is empty; refusing to guess a port/,
  );
});

test("an empty env override raises instead of falling through to a default", () => {
  assert.throws(
    () =>
      resolveHarnessDaemonBase({
        env: { [MESH_DAEMON_BASE_ENV]: "" },
        instanceBase: () => SUPPLIED_INSTANCE_BASE,
      }),
    /is empty; refusing to guess a port/,
  );
});

test("a non-URL base raises", () => {
  assert.throws(
    () => resolveHarnessDaemonBase({ explicit: "127.0.0.1:7786" }),
    /not an absolute URL/,
  );
});

test("a portless base raises rather than assuming one", () => {
  assert.throws(
    () => resolveHarnessDaemonBase({ explicit: "http://127.0.0.1/" }),
    /must carry an explicit port/,
  );
});

for (const port of [7685, 7686]) {
  test(`port ${port} is refused as a harness daemon base`, () => {
    assert.throws(
      () => resolveHarnessDaemonBase({ explicit: `http://127.0.0.1:${port}` }),
      new RegExp(`refusing harness daemon base on shared instance port ${port}`),
    );
  });
}

test("a valid base is normalized to its origin", () => {
  assert.equal(
    assertHarnessDaemonBase("http://127.0.0.1:7786/api/", "unit test"),
    "http://127.0.0.1:7786",
  );
});
