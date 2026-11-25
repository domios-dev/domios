import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultConfig } from "../../hooks/useConfig";
import type { AppConfig, ProviderDescriptor } from "../../lib/types";

const fixture = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(async () => () => {}),
  localFetch: vi.fn(),
  localJson: vi.fn(),
  peerRequest: vi.fn(),
  peerJson: vi.fn(),
  autostartGet: vi.fn(async () => true),
  autostartSet: vi.fn(async () => {}),
  openFolder: vi.fn(async () => {}),
}));

vi.mock("../../lib/bridge", () => ({
  invoke: fixture.invoke,
  listen: fixture.listen,
  checkProviderIntegration: (providerId: string, peerId?: string) =>
    fixture.invoke("check_provider_integration", peerId ? { provider_id: providerId, peerId } : { provider_id: providerId }),
  installProviderIntegration: (providerId: string) =>
    fixture.invoke("install_provider_integration", { provider_id: providerId, consent: true }),
  uninstallProviderIntegration: (providerId: string) =>
    fixture.invoke("uninstall_provider_integration", { provider_id: providerId }),
}));
vi.mock("../../lib/nativeBridge", () => ({
  nativeBridge: {
    autostart: { get: fixture.autostartGet, set: fixture.autostartSet },
    openFolder: fixture.openFolder,
    restartApp: vi.fn(async () => {}),
  },
}));
vi.mock("../../lib/webApi", async (original) => ({
  ...(await original<typeof import("../../lib/webApi")>()),
  localFetch: fixture.localFetch,
  localJson: fixture.localJson,
  daemonHttpBase: () => "http://127.0.0.1:7721",
  daemonAuthToken: () => "local-token",
}));
vi.mock("../../lib/daemonBootstrap", async (original) => ({
  ...(await original<typeof import("../../lib/daemonBootstrap")>()),
  daemonTransportForPeer: (peerId?: string) => ({
    request: (path: string, init?: RequestInit) => fixture.peerRequest(peerId, path, init),
    json: (method: string, path: string, body?: unknown) => fixture.peerJson(peerId, method, path, body),
  }),
}));
vi.mock("../../lib/peerEventRail", () => ({
  peerEventRail: { observe: () => () => {} },
}));
vi.mock("../../lib/productAnalytics", () => ({ recordProductEvent: vi.fn(async () => {}) }));
vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxSelector: () => [],
  useMuxState: () => ({ workspaces: [], tabs: [], teams: [] }),
  useMultiplexer: () => ({ selectTabByTab: vi.fn(), upsertToast: vi.fn() }),
}));
vi.mock("../../hooks/useUpdate", () => ({
  useUpdate: () => ({
    status: "idle",
    currentVersion: "1.23.3",
    latestVersion: null,
    releaseNotes: null,
    releaseUrl: null,
    refresh: vi.fn(async () => {}),
    applyUpdate: vi.fn(async () => {}),
    installing: false,
    downloadedBytes: 0,
    downloadTotalBytes: null,
    error: null,
  }),
}));

import { setWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { SettingsPanel } from "./SettingsPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as Record<string, unknown>).__APP_VERSION__ = "1.23.3";

const REMOTE_PEER = { id: "r161", name: "BLACKSTAR #1" };

const claudeProvider: ProviderDescriptor = {
  id: "claude",
  display_name: "Claude Code",
  transport: "pty",
  availability: "available",
  icon_svg: "",
  available_models: [{ id: "claude-sonnet-4-6", display_name: "Sonnet 4.6" }],
  default_model: "claude-sonnet-4-6",
  has_hooks: true,
  accounts: true,
  custom_endpoints: true,
  settings_schema: [
    { kind: "path_link", label: "Config file", path: "/remote/claude.json" },
    { kind: "action", id: "install-rule", label: "Install rule", command: "install_provider_rule", args: { provider_id: "claude" }, style: "primary" },
  ],
  notifications_schema: [],
} as unknown as ProviderDescriptor;

const codexProvider: ProviderDescriptor = {
  id: "codex",
  display_name: "Codex",
  transport: "pty",
  availability: "available",
  icon_svg: "",
  available_models: [{ id: "gpt-5.4", display_name: "GPT-5.4" }],
  default_model: "gpt-5.4",
  has_hooks: true,
  settings_schema: [],
  notifications_schema: [],
} as unknown as ProviderDescriptor;

const acpProvider: ProviderDescriptor = {
  id: "acp",
  display_name: "ACP",
  transport: "chat_backend",
  availability: "available",
  icon_svg: "",
  available_models: [],
  default_model: "",
  has_hooks: false,
  agent_runtime: { launchProfiles: { default: { label: "Default" } } },
  settings_schema: [],
  notifications_schema: [],
} as unknown as ProviderDescriptor;

function makeConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  const base = defaultConfig();
  return {
    ...base,
    ...overrides,
    general_agent: { ...base.general_agent, ...(overrides.general_agent ?? {}) },
    memory: { ...base.memory, ...(overrides.memory ?? {}) },
    terminal: { ...base.terminal, ...(overrides.terminal ?? {}) },
    integrations: { ...base.integrations, ...(overrides.integrations ?? {}) },
    updater: { ...base.updater, ...(overrides.updater ?? {}) },
  };
}

let host: HTMLDivElement;
let root: Root;
let localConfig: AppConfig;
let remoteConfig: AppConfig;
let onLocalUpdate: ReturnType<typeof vi.fn<(next: AppConfig) => Promise<void>>>;
let remoteConfigError: Error | null;

beforeEach(() => {
  setWorkspaceBrowseTarget(null);
  localConfig = makeConfig({
    general_agent: { ...defaultConfig().general_agent, enabled: true, agent_provider: "codex" },
    memory: { ...defaultConfig().memory, librarian_enabled: true },
    updater: { ...defaultConfig().updater, auto_update_provider_clis: true },
  });
  remoteConfig = makeConfig({
    general_agent: { ...defaultConfig().general_agent, enabled: true, agent_provider: "codex" },
    memory: { ...defaultConfig().memory, librarian_enabled: true },
    terminal: { ...defaultConfig().terminal, default_shell: "pwsh" },
    integrations: { ...defaultConfig().integrations, telegram_allowed_users: ["42"] },
    updater: { ...defaultConfig().updater, auto_update_provider_clis: true },
  });
  remoteConfigError = null;
  onLocalUpdate = vi.fn(async () => {});
  fixture.autostartGet.mockReset().mockResolvedValue(true);
  fixture.autostartSet.mockReset().mockResolvedValue(undefined);
  fixture.openFolder.mockReset().mockResolvedValue(undefined);
  fixture.localFetch.mockReset().mockImplementation(async (path: string) => {
    if (path.startsWith("/api/mem/embedder/providers")) {
      return new Response(JSON.stringify({
        providers: [],
        remote_opt_in: false,
        status: { provider: "local", model: "local-embed", dimension: 384, available: true, remote: false, pending: 0, reembed: { state: "idle", total: 0, done: 0, failed: 0 } },
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  });
  fixture.localJson.mockReset().mockResolvedValue({ consent: "off" });
  fixture.peerRequest.mockReset().mockImplementation(async (_peerId: string, path: string) => {
    if (path.startsWith("/api/mem/embedder/providers")) {
      return new Response(JSON.stringify({
        providers: [],
        remote_opt_in: false,
        status: { provider: "local", model: "remote-embed", dimension: 384, available: true, remote: false, pending: 0, reembed: { state: "idle", total: 0, done: 0, failed: 0 } },
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    if (path.startsWith("/api/logs")) {
      return new Response(JSON.stringify([
        { id: 1, ts: Date.now(), level: "info", source: "remote-r161", message: "Remote log entry", context_json: null },
      ]), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return new Response(JSON.stringify({}), { status: 200, headers: { "Content-Type": "application/json" } });
  });
  fixture.peerJson.mockReset().mockResolvedValue({ channels: [] });
  fixture.invoke.mockReset().mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === "get_config") {
      if (remoteConfigError) throw remoteConfigError;
      return structuredClone(remoteConfig);
    }
    if (command === "update_config") {
      remoteConfig = structuredClone(args?.config as AppConfig);
      return { ok: true };
    }
    if (command === "restart_general_agent") return { ok: true };
    if (command === "list_agent_providers") return [codexProvider, claudeProvider, acpProvider];
    if (command === "shell_profiles") return [{ id: "pwsh", name: "PowerShell 7" }, { id: "bash", name: "Bash" }];
    if (command === "push_channel_roster") return { channels: [] };
    if (command === "check_provider_integration") return { integration_state: "absent" };
    if (command === "provider_cli_status") return { required: true, installed: true, availability: "available", binary: "claude", resolved_path: "/usr/bin/claude", searched: [], installer: null, updater: null };
    if (command === "list_themes") return ["github-dark"];
    if (command === "get_themes_dir") return "";
    if (command === "get_shell_diagnostics") return { platform: "windows", pwsh_installed: false, current_shell: "powershell" };
    return {};
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  setWorkspaceBrowseTarget(null);
  host.remove();
});

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function renderSettings(initialSection?: React.ComponentProps<typeof SettingsPanel>["initialSection"]) {
  await act(async () => {
    root.render(
      <SettingsPanel
        config={localConfig}
        onUpdateConfig={onLocalUpdate}
        onClose={() => {}}
        initialSection={initialSection}
        restoreNavigation={false}
      />,
    );
  });
  await flush();
}

describe("Settings remote target routing", () => {
  it("routes General Agent provider switch, restart, config toggles, shell, and embedder to the browsed remote machine without mutating local", async () => {
    setWorkspaceBrowseTarget(REMOTE_PEER);
    await renderSettings("general");

    expect(host.querySelector('[data-testid="settings-remote-target"]')?.textContent).toBe("BLACKSTAR #1");
    expect(fixture.invoke).toHaveBeenCalledWith("get_config", { peerId: "r161" });
    expect(fixture.invoke).toHaveBeenCalledWith("list_agent_providers", { metadataOnly: true, peerId: "r161" });
    expect(fixture.invoke).toHaveBeenCalledWith("shell_profiles", { refresh: false, peerId: "r161" });
    expect(fixture.peerRequest).toHaveBeenCalledWith("r161", "/api/mem/embedder/providers", expect.anything());
    // Local-only reads must not fire while browsing a remote machine.
    expect(fixture.localJson).not.toHaveBeenCalledWith("GET", "/api/analytics/status");
    expect(fixture.invoke).not.toHaveBeenCalledWith("get_shell_diagnostics", expect.anything());

    // Local-only controls are disabled.
    const autostart = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Launch Domios on system start"]');
    expect(autostart?.disabled).toBe(true);
    const analytics = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Share product analytics"]');
    expect(analytics?.disabled).toBe(true);
    expect(host.querySelector('[data-testid="settings-telegram-connect"]')).toBeNull();

    // D1 exact flow: switch General Agent provider to Claude Code -> "Change and restart".
    const claudeCard = [...host.querySelectorAll<HTMLButtonElement>("button")].find((btn) =>
      btn.textContent?.includes("Claude Code"),
    );
    expect(claudeCard).toBeDefined();
    await act(async () => claudeCard!.click());

    const confirmBtn = [...document.querySelectorAll<HTMLButtonElement>("button")].find((btn) =>
      btn.textContent?.includes("Change and restart"),
    );
    expect(confirmBtn).toBeDefined();
    await act(async () => confirmBtn!.click());
    await flush();

    const updateCalls = fixture.invoke.mock.calls.filter(([cmd]) => cmd === "update_config");
    expect(updateCalls).toHaveLength(1);
    expect(updateCalls[0][1]).toMatchObject({
      peerId: "r161",
      config: expect.objectContaining({
        general_agent: expect.objectContaining({ agent_provider: "claude" }),
      }),
    });
    expect(fixture.invoke).toHaveBeenCalledWith("restart_general_agent", { peerId: "r161" });
    expect(onLocalUpdate).not.toHaveBeenCalled();

    // Direct General Agent Restart button also targets r161.
    const restartBtn = host.querySelector<HTMLButtonElement>('[data-testid="settings-general-agent-restart"]');
    expect(restartBtn).not.toBeNull();
    await act(async () => restartBtn!.click());
    const restartCalls = fixture.invoke.mock.calls.filter(([cmd]) => cmd === "restart_general_agent");
    expect(restartCalls).toEqual([
      ["restart_general_agent", { peerId: "r161" }],
      ["restart_general_agent", { peerId: "r161" }],
    ]);

    // Config toggle (Librarian) writes to r161, never local.
    const librarian = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Enable Librarian (background skill vectorizer)"]');
    expect(librarian).not.toBeNull();
    await act(async () => librarian!.click());
    expect(fixture.invoke.mock.calls.filter(([cmd]) => cmd === "update_config").at(-1)?.[1]).toMatchObject({
      peerId: "r161",
      config: expect.objectContaining({ memory: expect.objectContaining({ librarian_enabled: false }) }),
    });
    expect(onLocalUpdate).not.toHaveBeenCalled();
    expect(fixture.autostartSet).not.toHaveBeenCalled();
  });

  it("routes Agents, Harness detail, Notifications, Logs, and About to the remote machine and disables local-only panes", async () => {
    setWorkspaceBrowseTarget(REMOTE_PEER);

    // Secrets: local-only, no backend select, no local /api/secrets fetch.
    await renderSettings("secrets");
    expect(host.querySelector('select[aria-label="Backend"]')).toBeNull();
    expect(host.querySelector('[data-testid="settings-remote-local-only"]')?.textContent).toContain("BLACKSTAR #1");
    expect(fixture.localFetch).not.toHaveBeenCalledWith(expect.stringContaining("/api/secrets"), expect.anything());

    // Account: local-only.
    await renderSettings("account");
    expect(host.querySelector('[data-testid="settings-remote-local-only"]')?.textContent).toContain("BLACKSTAR #1");

    // Mesh: local-only.
    await renderSettings("mesh");
    expect(host.querySelector('[data-testid="settings-remote-local-only"]')?.textContent).toContain("BLACKSTAR #1");

    // Notifications: routes push_channel_roster and config toggles to r161.
    await renderSettings("notifications");
    expect(fixture.invoke).toHaveBeenCalledWith("push_channel_roster", { peerId: "r161" });
    const bellSwitch = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Show the bell"]');
    expect(bellSwitch).not.toBeNull();
    await act(async () => bellSwitch!.click());
    expect(fixture.invoke.mock.calls.filter(([cmd]) => cmd === "update_config").at(-1)?.[1]).toMatchObject({
      peerId: "r161",
    });
    expect(onLocalUpdate).not.toHaveBeenCalled();

    // Logs: reads from peer transport, never localFetch.
    await renderSettings("logs");
    expect(fixture.peerRequest).toHaveBeenCalledWith("r161", expect.stringMatching(/^\/api\/logs\?/), expect.anything());
    expect(fixture.localFetch).not.toHaveBeenCalledWith(expect.stringContaining("/api/logs"), expect.anything());
    expect(host.textContent).toContain("Remote log entry");

    // Check for updates (About): CLI auto-update toggle writes to r161, app updates disabled.
    await renderSettings("about");
    const autoCliToggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Update harness CLIs automatically"]');
    expect(autoCliToggle).not.toBeNull();
    await act(async () => autoCliToggle!.click());
    expect(fixture.invoke.mock.calls.filter(([cmd]) => cmd === "update_config").at(-1)?.[1]).toMatchObject({
      peerId: "r161",
      config: expect.objectContaining({ updater: expect.objectContaining({ auto_update_provider_clis: false }) }),
    });
    expect([...host.querySelectorAll("button")].some((btn) => btn.textContent?.includes("Check again"))).toBe(false);
    expect(host.querySelector('[data-testid="settings-remote-local-only"]')?.textContent).toContain("BLACKSTAR #1");

    // Agents overview: lists remote providers, routes team channels to peer, disables Spinal Cord & Open skills folder.
    await renderSettings("agents");
    expect(fixture.peerJson).toHaveBeenCalledWith("r161", "GET", "/api/team-channels", undefined);
    const openSkills = [...host.querySelectorAll<HTMLButtonElement>("button")].find((btn) =>
      btn.textContent?.includes("Open skills folder"),
    );
    expect(openSkills?.disabled).toBe(true);
    expect(fixture.localFetch).not.toHaveBeenCalledWith(expect.stringContaining("/api/supporter"), expect.anything());

    // Open Claude Code harness detail screen.
    const claudeRow = host.querySelector<HTMLButtonElement>('[data-testid="settings-agent-provider-row"][data-provider-id="claude"]');
    expect(claudeRow).not.toBeNull();
    await act(async () => claudeRow!.click());
    await flush();

    // Integration health checks remote peer and disables Repair/Install.
    expect(fixture.invoke).toHaveBeenCalledWith("check_provider_integration", { provider_id: "claude", peerId: "r161" });
    const repairBtn = host.querySelector<HTMLButtonElement>('[data-provider-integration-action="repair"]');
    expect(repairBtn?.disabled).toBe(true);
    expect(fixture.invoke).not.toHaveBeenCalledWith("install_provider_integration", expect.anything());

    // Schema path_link is hidden and schema action is disabled.
    expect(host.querySelector('[data-testid="settings-schema-open-folder"]')).toBeNull();
    const schemaAction = host.querySelector<HTMLButtonElement>('[data-testid="settings-schema-action"]');
    expect(schemaAction?.disabled).toBe(true);

    // Visibility toggle still writes to r161.
    const visibility = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Show in workspace menu"]');
    expect(visibility).not.toBeNull();
    await act(async () => visibility!.click());
    expect(fixture.invoke.mock.calls.filter(([cmd]) => cmd === "update_config").at(-1)?.[1]).toMatchObject({
      peerId: "r161",
    });
    expect(onLocalUpdate).not.toHaveBeenCalled();

    // Back to Agents overview -> open ACP harness detail -> launch profiles & registry are local-only.
    const backBtn = host.querySelector<HTMLButtonElement>('[data-testid="settings-back"]');
    expect(backBtn).not.toBeNull();
    await act(async () => backBtn!.click());
    await flush();
    const acpRow = host.querySelector<HTMLButtonElement>('[data-testid="settings-agent-provider-row"][data-provider-id="acp"]');
    expect(acpRow).not.toBeNull();
    await act(async () => acpRow!.click());
    await flush();
    expect(host.querySelector('[data-testid="settings-acp-profiles"]')).toBeNull();
    expect(host.querySelector('[data-testid="settings-remote-local-only"]')?.textContent).toContain("BLACKSTAR #1");
    expect(fixture.invoke).not.toHaveBeenCalledWith("provider_launch_profiles", expect.anything());
  });

  it("refuses to show or mutate local config when remote config fails to load", async () => {
    setWorkspaceBrowseTarget(REMOTE_PEER);
    remoteConfigError = new Error("peer offline");
    await renderSettings("general");

    expect(host.querySelector('[data-testid="settings-general-agent-restart"]')).toBeNull();
    expect(host.textContent).toContain("BLACKSTAR #1");
    expect(onLocalUpdate).not.toHaveBeenCalled();
  });

  it("preserves local Settings writes and restarts when no remote machine is browsed", async () => {
    setWorkspaceBrowseTarget(null);
    await renderSettings("general");

    expect(host.querySelector('[data-testid="settings-remote-target"]')).toBeNull();
    const claudeCard = [...host.querySelectorAll<HTMLButtonElement>("button")].find((btn) =>
      btn.textContent?.includes("Claude Code"),
    );
    expect(claudeCard).toBeDefined();
    await act(async () => claudeCard!.click());

    const confirmBtn = [...document.querySelectorAll<HTMLButtonElement>("button")].find((btn) =>
      btn.textContent?.includes("Change and restart"),
    );
    await act(async () => confirmBtn!.click());
    await flush();

    expect(onLocalUpdate).toHaveBeenCalledTimes(1);
    expect(onLocalUpdate.mock.calls[0][0].general_agent.agent_provider).toBe("claude");
    expect(fixture.invoke).toHaveBeenCalledWith("restart_general_agent", undefined);
    expect(fixture.invoke).not.toHaveBeenCalledWith("update_config", expect.anything());
  });
});
