import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { configureDaemonTransport } from "./daemonBootstrap";
import { cachedProviderIcon, persistProviderIcons, subscribeProviderIcons } from "./providerIconCache";

test("icon metadata hydrates synchronously, stays viewer-scoped, and replaces removed providers", () => {
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
  configureDaemonTransport({ origin: "http://localhost", token: "fixture" });
  const key = "codeterm-provider-icons:http://localhost:self";
  const icon = { id: "fixture", display_name: "Fixture", icon_svg: "<svg/>" };
  localStorage.setItem(key, JSON.stringify([icon]));
  assert.deepEqual(cachedProviderIcon("fixture"), icon);
  localStorage.setItem("codeterm-web-relay-peer:http://localhost", "peer");
  assert.equal(cachedProviderIcon("fixture"), undefined);
  localStorage.removeItem("codeterm-web-relay-peer:http://localhost");
  assert.deepEqual(cachedProviderIcon("fixture"), icon);
  persistProviderIcons([]);
  assert.equal(cachedProviderIcon("fixture"), undefined);
  assert.deepEqual(JSON.parse(localStorage.getItem(key)!), []);
});

test("unchanged icon descriptors skip storage writes and subscriber notifications", () => {
  const stored = new Map<string, string>();
  let writes = 0;
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => { writes++; stored.set(key, value); },
    removeItem: (key: string) => stored.delete(key),
  } });
  let notifications = 0;
  const unsubscribe = subscribeProviderIcons(() => { notifications++; });
  try {
    const icon = { id: "stable", display_name: "Stable", icon_svg: "<svg/>" };
    persistProviderIcons([icon]);
    assert.equal(writes, 1);
    assert.equal(notifications, 1);
    persistProviderIcons([{ ...icon }]);
    assert.equal(writes, 1);
    assert.equal(notifications, 1);
    persistProviderIcons([{ ...icon, icon_svg: "<svg><path/></svg>" }]);
    assert.equal(writes, 2);
    assert.equal(notifications, 2);
  } finally {
    unsubscribe();
  }
});

test("corrupt and refused storage never blocks icons", () => {
  localStorage.setItem("codeterm-web-relay-peer:http://localhost", "corrupt");
  localStorage.setItem("codeterm-provider-icons:http://localhost:corrupt", "broken");
  assert.equal(cachedProviderIcon("fixture"), undefined);
  Object.defineProperty(globalThis, "localStorage", { configurable: true, get() { throw new Error("denied"); } });
  assert.doesNotThrow(() => persistProviderIcons([]));
  assert.doesNotThrow(() => cachedProviderIcon("fixture"));
});
