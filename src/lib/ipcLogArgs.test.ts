import assert from "node:assert/strict";
import { test } from "node:test";
import { ipcLogArgs } from "./ipcLogArgs";

test("secret-bearing IPC fields never reach a log line", () => {
  const key = ["opaque", "fixture", "key"].join("-");
  const logged = ipcLogArgs({ providerId: "fixture", request: { action: "add", value: key, provider: { id: "demo" } } });
  assert.equal(JSON.stringify(logged).includes(key), false);
  assert.deepEqual(logged, { providerId: "fixture", request: { action: "add", value: "[redacted]", provider: { id: "demo" } } });

  const code = ["pkce", "single", "use", "code"].join("-");
  const loginLogged = ipcLogArgs({
    tabId: "mesh:4974b28eec1a4248bc44693b2d8c4afb/004c7c5c",
    request: { action: "code", code, loginId: "flow-1" },
  });
  assert.equal(JSON.stringify(loginLogged).includes(code), false);
  assert.deepEqual(loginLogged, {
    tabId: "mesh:4974b28eec1a4248bc44693b2d8c4afb/004c7c5c",
    request: { action: "code", code: "[redacted]", loginId: "flow-1" },
  });
});

test("ordinary arguments are logged unchanged", () => {
  const args = { tabId: "t1", items: [{ name: "n" }], count: 3, absent: undefined };
  assert.deepEqual(ipcLogArgs(args), args);
});
