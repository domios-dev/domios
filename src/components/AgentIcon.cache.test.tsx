import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { configureDaemonTransport } from "../lib/daemonBootstrap";
import { persistProviderIcons } from "../lib/providerIconCache";
import { AgentIcon } from "./AgentIcon";

test("cached logo is visible on first render and an authoritative removal restores the fallback", async () => {
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
  configureDaemonTransport({ origin: "http://localhost", token: "fixture" });
  localStorage.setItem("codeterm-provider-icons:http://localhost:self", JSON.stringify([
    { id: "cached", display_name: "Cached", icon_svg: "<svg><path /></svg>" },
  ]));
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AgentIcon providerId="cached" />));
    assert.ok(host.querySelector('[role="img"][aria-label="Agent logo"]'));
    await act(async () => persistProviderIcons([]));
    assert.ok(host.querySelector('[role="img"][aria-label="Default agent icon"]'));
  } finally {
    await act(async () => root.unmount());
  }
});
