// Run: npx tsx src/components/Sidebar/tabModelMenu.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { ModelInfo, ProviderDescriptor, Tab } from "../../lib/types";
import {
  switchTabModel,
  tabModelEntryLabel,
  tabModelMenuView,
  tabModelSwitchPayload,
} from "./tabModelMenu";
import { __setTabModelApiForTest } from "./tabModelApi";

const MODELS: ModelInfo[] = [
  { id: "fixture-small", display_name: "Fixture Small" },
  { id: "fixture-large", display_name: "Fixture Large" },
];

function descriptor(overrides: Partial<ProviderDescriptor> = {}): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "fixture-provider",
    display_name: "Fixture Provider",
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: MODELS,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    model_switch_format: "/switch {model}",
    ...overrides,
  } as ProviderDescriptor;
}

function tab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "tab",
    pty_id: "pty-1",
    tab_type: "Terminal",
    title: "Main",
    cwd: null,
    agent_type: "fixture-provider",
    ...overrides,
  };
}

describe("which rows offer a model switch", () => {
  it("offers it to a PTY agent whose provider publishes a switch command and models", () => {
    expect(tabModelMenuView(tab(), descriptor(), null)).not.toBeNull();
  });

  it("offers it to the protected General Agent row — protection guards close and rename, not the model", () => {
    expect(tabModelMenuView(tab({ protected: true }), descriptor(), null)).not.toBeNull();
  });

  it("withholds it from a plain terminal tab", () => {
    expect(tabModelMenuView(tab({ agent_type: null }), descriptor(), null)).toBeNull();
  });

  it("withholds it from a chatBackend shell, which owns its own model bubble", () => {
    const shell = tab({ backing: { kind: "chat_backend", plugin_id: "fixture-provider", session_id: "s" } });
    expect(tabModelMenuView(shell, descriptor(), null)).toBeNull();
  });

  it("withholds it when the provider publishes no in-agent switch command", () => {
    expect(tabModelMenuView(tab(), descriptor({ model_switch_format: null }), null)).toBeNull();
    expect(tabModelMenuView(tab(), descriptor({ model_switch_format: "  " }), null)).toBeNull();
  });

  it("withholds it when the provider publishes no models", () => {
    expect(tabModelMenuView(tab(), descriptor({ available_models: [] }), null)).toBeNull();
  });

  it("withholds it when the provider explicitly declares no model surface", () => {
    expect(tabModelMenuView(tab(), descriptor({ behavior_config: { model: "none" } }), null)).toBeNull();
  });

  it("keeps it when behavior_config is absent, which is not a refusal", () => {
    expect(tabModelMenuView(tab(), descriptor({ behavior_config: undefined }), null)).not.toBeNull();
  });

  it("withholds it when the descriptor belongs to another provider", () => {
    expect(tabModelMenuView(tab(), descriptor({ id: "someone-else" }), null)).toBeNull();
  });

  it("withholds it without a live PTY to write the switch into", () => {
    expect(tabModelMenuView(tab({ pty_id: null }), descriptor(), null)).toBeNull();
  });
});

describe("the entry reflects session state", () => {
  it("marks the published model this session reports", () => {
    const view = tabModelMenuView(tab({ agent_session_model: "fixture-large" }), descriptor(), null);
    expect(view?.currentModel?.id).toBe("fixture-large");
  });

  it("matches a provider-normalised session model against the published id", () => {
    const view = tabModelMenuView(tab({ agent_session_model: "Fixture-Large" }), descriptor(), null);
    expect(view?.currentModel?.id).toBe("fixture-large");
  });

  it("marks nothing when the session model is not one the provider published", () => {
    const view = tabModelMenuView(tab({ agent_session_model: "retired-model" }), descriptor(), null);
    expect(view?.currentModel).toBeNull();
  });

  it("names the entry from the descriptor, so a renamed model renames the control", () => {
    const renamed = descriptor({
      available_models: [{ id: "fixture-large", display_name: "Fixture Large mk2" }],
    });
    const view = tabModelMenuView(tab({ agent_session_model: "fixture-large" }), renamed, null);
    expect(tabModelEntryLabel(view!)).toBe("Model: Fixture Large mk2");
  });

  it("blocks the entry with a reason while a turn is in flight, rather than interrupting it", () => {
    const working = tabModelMenuView(tab(), descriptor(), "Working");
    const idle = tabModelMenuView(tab(), descriptor(), "Idle");
    expect(typeof working?.blockedReason).toBe("string");
    expect(idle?.blockedReason).toBeNull();
  });
});

describe("the switch round-trips the id it was given", () => {
  it("formats the payload from the provider's own template", () => {
    const view = tabModelMenuView(tab(), descriptor({ model_switch_format: "!use {model} now" }), null);
    expect(tabModelSwitchPayload(view!, "fixture-large")).toBe("!use fixture-large now\r");
  });

  it("appends the id when the template names no placeholder", () => {
    const view = tabModelMenuView(tab(), descriptor({ model_switch_format: "/model" }), null);
    expect(tabModelSwitchPayload(view!, "fixture-small")).toBe("/model fixture-small\r");
  });

  it("writes the selected id to this tab's PTY", async () => {
    const calls: Array<[string, string]> = [];
    __setTabModelApiForTest({
      switchModel: async (ptyId, payload) => {
        calls.push([ptyId, payload]);
      },
    });
    try {
      const view = tabModelMenuView(tab({ pty_id: "pty-fixture" }), descriptor(), null);
      await switchTabModel(view!, MODELS[1].id);
      expect(calls.length).toBe(1);
      expect(calls[0][0]).toBe("pty-fixture");
      expect(calls[0][1]).toBe(`/switch ${MODELS[1].id}\r`);
    } finally {
      __setTabModelApiForTest(null);
    }
  });
});

finish();
