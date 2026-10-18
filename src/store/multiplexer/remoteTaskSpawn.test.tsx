import "../../components/Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import {test} from "node:test";
import {act} from "react";
import {createRoot} from "react-dom/client";
import {__setInvokeForTest, __setListenForTest} from "../../lib/bridge";
import {setAgentDescriptors} from "../../lib/agentDescriptorCache";
import {MultiplexerProvider, useMultiplexer} from "./index";
import type {MultiplexerActionsContextType} from "./context";
import type {ProviderDescriptor} from "../../lib/types";

test("remote environment reaches chat-backend config rather than local model-preset resolution", async () => {
  const provider: ProviderDescriptor = {
    schema_version: 1, id: "remote-fixture", display_name: "Remote fixture", transport: "chat_backend", system_prompt_delivery: "external",
    available_models: [], settings_schema: [], notifications_schema: [], supports_lazy_skills: false, has_hooks: false,
    remote_task: {backend: "fixture", mode: "cloud", displayName: "Cloud", environments: [], availability: {status: "available"}, followUp: false, cancel: false, applyDiff: false},
  };
  const snapshot = {workspaces: [{id: "workspace", name: "Fixture", tabs: [], active_tab_id: null, cwd: "/repo"}], tabs: {}, teams: [], active_workspace_id: "workspace", focused_tab_id: null, generation: 1};
  const requests: unknown[] = [];
  let actions!: MultiplexerActionsContextType;
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "mux_get_state") return snapshot as T;
    if (command === "mux_spawn_tab") {
      requests.push(args);
      return {workspace_id: "workspace", tab_id: "remote-tab", snapshot} as T;
    }
    if (command === "get_config") return {api: {enabled: false}, agent: {}} as T;
    if (command === "get_mesh_peers") return {peers: []} as T;
    if (["list_notifications", "ordinary_tabs_list", "list_agent_providers"].includes(command)) return [] as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  function Capture() {actions = useMultiplexer(); return null;}
  const host = document.createElement("div"); const root = createRoot(host);
  try {
    await act(async () => root.render(<MultiplexerProvider><Capture /></MultiplexerProvider>));
    setAgentDescriptors([provider], false);
    await act(async () => {await actions.spawnAgentTab("workspace", "remote-fixture", "/repo", undefined, undefined, "env-fixture", undefined, "cloud");});
    assert.deepEqual(requests, [{params: {agentId: "remote-fixture", workspaceId: "workspace", cwd: "/repo", model: undefined, reasoningEffort: undefined, config: {preset: "env-fixture", executionMode: "cloud"}}}]);
  } finally {
    await act(async () => root.unmount());
    setAgentDescriptors([], false);
    __setInvokeForTest(null); __setListenForTest(null);
  }
});
