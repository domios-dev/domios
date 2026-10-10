import { useMemo, type ReactNode } from "react";
import claudeManifest from "../../../plugins/provider-claude/plugin.json";
import claudeIcon from "../../../plugins/provider-claude/icon.svg?raw";
import codexManifest from "../../../plugins/provider-codex/plugin.json";
import codexIcon from "../../../plugins/provider-codex/icon.svg?raw";
import { defaultConfig } from "../../../src/hooks/useConfig";
import { setAgentDescriptors } from "../../../src/lib/agentDescriptorCache";
import { daemonHttpBase } from "../../../src/lib/webApi";
import type { ApiConfig, AppConfig, ModelSwitchMode, ProviderDescriptor, Tab, TabStatus, Team } from "../../../src/lib/types";
import { MultiplexerActionsContext, MuxStateStoreContext, type MultiplexerActionsContextType } from "../../../src/store/multiplexer/context";
import { createMuxStateStore } from "../../../src/store/multiplexer/selectorStore";
import type { ExtendedState } from "../../../src/store/multiplexer/tree";
import { PlansProvider } from "../../../src/components/Canvas/PlansProvider";
import { mockCommand, mockHttp, provideList } from "../mocks/backend";

type Manifest = {
  id: string;
  displayName: string;
  iconColor?: string;
  models?: Array<{ id: string; displayName: string; description?: string; primary?: boolean; group?: string }>;
  defaultModel?: string;
  reasoningEfforts?: Array<{ id: string; displayName: string; description?: string }>;
  capabilities?: { modelSwitch?: string };
};

function descriptorFromManifest(manifest: Manifest, iconSvg: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id: manifest.id,
    display_name: manifest.displayName,
    transport: "pty",
    system_prompt_delivery: "external",
    icon_svg: iconSvg,
    icon_color: manifest.iconColor,
    available_models: (manifest.models ?? []).map((model) => ({
      id: model.id,
      display_name: model.displayName,
      description: model.description,
      primary: model.primary ?? null,
      group: model.group ?? null,
    })),
    default_model: manifest.defaultModel,
    reasoning_efforts: (manifest.reasoningEfforts ?? []).map((effort) => ({ id: effort.id, display_name: effort.displayName, description: effort.description })),
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: true,
    supports_chat_ui: true,
    supports_permission_auto_confirm: true,
    model_switch: (manifest.capabilities?.modelSwitch ?? "restart_resume") as ModelSwitchMode,
  };
}

export const CLAUDE = descriptorFromManifest(claudeManifest as unknown as Manifest, claudeIcon);
export const CODEX = descriptorFromManifest(codexManifest as unknown as Manifest, codexIcon);

setAgentDescriptors([CLAUDE, CODEX]);
mockCommand("list_agent_providers", () => [CLAUDE, CODEX]);
mockCommand("plugin_tab_menu", () => ({ items: [], receiverTargets: {} }));
mockCommand("bridge_thread_state", () => ({ bridge_active: false, topics: "unavailable", bound: false }));

export const WORKSPACE_ID = "gallery-workspace";

export const TEAM: Team = {
  id: "gallery-team",
  name: "Gallery refresh",
  lifecycle: "active",
  created_at: 1,
  workspace_id: WORKSPACE_ID,
};

export function agentTab(id: string, overrides: Partial<Tab> = {}): Tab {
  return {
    id,
    workspace_id: WORKSPACE_ID,
    pty_id: `pty-${id}`,
    tab_type: "Terminal",
    title: "Agent",
    cwd: "/Developer/codeterm",
    agent_type: CLAUDE.id,
    agent_session_id: "4f1c2a9e-7b3d-4e21-9c55-0a8d3f6b1e72",
    agent_session_model: "opus",
    role: null,
    role_profile: null,
    parent_tab_id: null,
    agent_state: { state: "idle" },
    ...overrides,
  };
}

export function shellTab(id: string, overrides: Partial<Tab> = {}): Tab {
  return {
    id,
    workspace_id: WORKSPACE_ID,
    pty_id: `pty-${id}`,
    tab_type: "Terminal",
    title: "zsh",
    cwd: "/Developer/codeterm",
    role: null,
    role_profile: null,
    parent_tab_id: null,
    ...overrides,
  };
}

export const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: null, cors: true };

/** A daemon API on its own origin, so one specimen state answers HTTP from its own mocks. */
export function httpApi(name: string): ApiConfig {
  return { ...API, host: `${name}.gallery.test` };
}

export function mockDaemon(api: ApiConfig, path: string, handler: Parameters<typeof mockHttp>[2]): void {
  mockHttp(daemonHttpBase(api), path, handler);
}

export function appConfig(api: ApiConfig = API): AppConfig {
  return { ...defaultConfig(), api };
}

export function muxState(tabs: Tab[], statuses: Record<string, TabStatus> = {}, workspaceId = WORKSPACE_ID): ExtendedState {
  return {
    workspaces: [{ id: workspaceId, name: "CodeTerm", tabs: tabs.map((tab) => tab.id), active_tab_id: tabs[0]?.id ?? null, cwd: "/Developer/codeterm" }],
    tabs: Object.fromEntries(tabs.map((tab) => [tab.id, tab])),
    teams: [TEAM],
    active_workspace_id: workspaceId,
    focused_tab_id: tabs[0]?.id ?? null,
    notifications: [],
    tabStatuses: statuses,
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

const actions = new Proxy({}, { get: () => async () => undefined }) as MultiplexerActionsContextType;

export function ProductProviders({ tabs = [], plans = false, workspaceId, children }: { tabs?: Tab[]; plans?: boolean; workspaceId?: string; children: ReactNode }) {
  const store = useMemo(() => {
    const state = muxState(tabs, {}, workspaceId);
    return createMuxStateStore(() => state);
  }, [tabs, workspaceId]);
  return <MultiplexerActionsContext.Provider value={actions}>
    <MuxStateStoreContext.Provider value={store}>
      {plans ? <PlansProvider api={API}>{children}</PlansProvider> : children}
    </MuxStateStoreContext.Provider>
  </MultiplexerActionsContext.Provider>;
}

for (const command of ["cron_list", "list_plans_full", "list_plan_index"]) provideList(command, []);
