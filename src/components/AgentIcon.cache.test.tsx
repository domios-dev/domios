import "../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { configureDaemonTransport } from "../lib/daemonBootstrap";
import { persistProviderIcons } from "../lib/providerIconCache";
import { AgentIcon } from "./AgentIcon";
import { setAgentDescriptors } from "../lib/agentDescriptorCache";
import type { ProviderDescriptor, Tab } from "../lib/types";

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

test("profile icons bypass the shared provider cache in tabs and profile choices", async () => {
  persistProviderIcons([{ id: "shared-runtime", display_name: "Harness", icon_svg: "<svg><path /></svg>" }]);
  const descriptor: ProviderDescriptor = { schema_version: 1, system_prompt_delivery: "external", available_models: [], settings_schema: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    id: "shared-runtime", display_name: "Harness", transport: "chat_backend", icon_svg: "<svg><path /></svg>", agent_runtime: { launchProfiles: {
    first: { binary: "first-agent", args: [], iconSvg: "<svg><circle /></svg>" },
    second: { binary: "second-agent", args: [] },
  } } };
  setAgentDescriptors([descriptor]);
  const profileTab = (profile: string): Tab => ({ id: profile, pty_id: null, tab_type: "Terminal", title: profile, cwd: null, backing: { kind: "pty" }, agent_type: "shared-runtime", agent_runtime_config: { launchProfile: profile }, slept_at_ms: 1, agent_runtime_state: null });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>
      <AgentIcon tab={profileTab("first")} providerId="shared-runtime" title="First profile" />
      <AgentIcon tab={profileTab("second")} providerId="shared-runtime" title="Second profile" />
      <AgentIcon profileId="second" providerId="shared-runtime" title="Profile choice" />
    </>));
    assert.ok(host.querySelector('[role="img"][aria-label="First profile"]'));
    assert.ok(host.querySelector('[role="img"][aria-label="Second profile default agent icon"]'));
    assert.ok(host.querySelector('[role="img"][aria-label="Profile choice default agent icon"]'));
    await act(async () => setAgentDescriptors([{ ...descriptor, agent_runtime: { launchProfiles: {
      ...descriptor.agent_runtime?.launchProfiles,
      second: { binary: "second-agent", args: [], iconSvg: "<svg><path /></svg>" },
    } } }]));
    assert.ok(host.querySelector('[role="img"][aria-label="Second profile"]'));
    assert.ok(host.querySelector('[role="img"][aria-label="Profile choice"]'));
  } finally {
    await act(async () => root.unmount());
    setAgentDescriptors([]);
  }
});
