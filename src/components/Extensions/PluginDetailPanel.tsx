// Right pane of the Extensions surface: the selected plugin's detail. Routes
// agent providers to AgentProviderScreen and secret backends to
// SecretBackendDetail (unchanged behavior); everything else gets the generic
// header + primary action + capabilities + settings + markdown description.
import { useState } from "react";
import {
  canUninstall,
  capabilityKeys,
  catalogKindLabel,
  primaryAction,
  type CatalogEntry,
} from "./pluginCatalog";
import { CapabilityChip, capabilityLabel } from "./CapabilityChip";
import { SchemaRenderer } from "../Settings/SchemaRenderer";
import { PresetManager } from "./PresetManager";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { AgentProviderScreen } from "../Settings/AgentsTab";
import { SecretBackendDetail } from "./SecretBackendDetail";
import { MarkdownDescription } from "./MarkdownDescription";
import { PluginIcon } from "./PluginIcon";
import { GroupLabel, iconOf, type AppConfigShape } from "./shared";
import { invoke } from "../../lib/bridge";
import { configureWithAI } from "../../lib/pluginConfigure";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { AppConfig, PluginActivity, ProviderConfig } from "../../lib/types";
import { resolvePluginActivityDisplay, type PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import { PluginActivityInline } from "./PluginActivityInline";

export type ActionKind = "install" | "update" | "remove";
export type ActionOptions = { removeData?: boolean };

export function PluginDetailPanel({
  entry,
  config,
  busy,
  onAction,
  onSave,
  onUpdateFullConfig,
  onGone,
  activity,
  activityDisplayMap,
}: {
  entry: CatalogEntry;
  config: AppConfigShape | null;
  busy: string | null;
  activity?: PluginActivity;
  activityDisplayMap: PluginActivityDisplayMap;
  onAction: (kind: ActionKind, entry: CatalogEntry, options?: ActionOptions) => void;
  onSave: (pluginId: string, key: string, value: unknown) => Promise<void>;
  onUpdateFullConfig: (next: AppConfig) => void | Promise<void>;
  onGone: () => void;
}) {
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [removeData, setRemoveData] = useState(true);
  // Capability-gated: a chatBackend whose behaviour declares presets gets the
  // manager. Keys off the transport-tagged descriptor, never a provider id.
  const descriptor = useAgentDescriptor(entry.id);
  const supportsPresets = descriptor?.behavior_config?.model === "presets_and_system_prompt";

  // Agent plugins render their real provider settings via the shared screen.
  if (entry.capabilities?.agent_provider && config) {
    return (
      <AgentProviderScreen
        providerId={entry.id}
        fullConfig={config as unknown as AppConfig}
        onUpdateFullConfig={(next) => void onUpdateFullConfig(next)}
        onProviderGone={onGone}
      />
    );
  }

  // Secret-backend plugins manage the secret store here (only meaningful once
  // installed; before install, fall through to the generic install view).
  if (entry.capabilities?.secret_backend && entry.installed && config) {
    return <SecretBackendDetail entry={entry} config={config} />;
  }

  const action = primaryAction(entry);
  const showUninstall = canUninstall(entry) && action.kind !== "uninstall";
  const busyKey = (k: ActionKind) => busy === `${k}:${entry.id}`;
  const anyBusy = busy?.endsWith(`:${entry.id}`) ?? false;

  const schema = entry.settingsSchema ?? [];
  const current = (config?.plugins?.[entry.id] ?? {}) as Record<string, unknown>;
  const perms = entry.grantedPermissions ?? [];
  const capKeys = capabilityKeys(entry.capabilities);

  const onPluginConfigChange = (id: string, patch: Partial<ProviderConfig>) => {
    for (const [k, v] of Object.entries(patch)) void onSave(id, k, v);
  };

  const beginRemove = () => {
    setRemoveData(true);
    setConfirmingRemove(true);
  };

  const runPrimary = () => {
    if (action.kind === "install") onAction("install", entry);
    else if (action.kind === "update") onAction("update", entry);
    else if (action.kind === "uninstall") beginRemove();
  };

  return (
    <div data-testid="extensions-plugin-detail" data-plugin-id={entry.id}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 14, marginBottom: 16 }}>
        <div style={{ width: 44, height: 44, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", flex: "none", background: "var(--ct-bg-elev)", border: "1px solid var(--ct-border-subtle)" }}>
          <PluginIcon plugin={iconOf(entry)} size={22} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 17, fontWeight: 640 }}>{entry.displayName}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, color: "var(--ct-muted)", marginTop: 3 }}>
            {entry.id} · {catalogKindLabel(entry)}
            {entry.version ? ` · v${entry.version}` : ""}
            {entry.updateAvailable && entry.remoteVersion ? ` → v${entry.remoteVersion}` : ""}
            {entry.dev ? " · dev (workspace)" : entry.channel ? ` · ${channelLabel(entry.channel)}` : ""}
          </div>
          <PluginActivityInline
            activity={activity}
            display={resolvePluginActivityDisplay(entry.id, activityDisplayMap)}
          />
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6, alignItems: "flex-end", flex: "none" }}>
          {action.kind === "none" ? (
            <span style={{ fontSize: 11.5, color: "var(--ct-muted)", padding: "5px 0" }}>{action.label}</span>
          ) : (
            <button
              data-testid={`extensions-plugin-${action.kind === "uninstall" ? "uninstall" : action.kind}`}
              data-plugin-id={entry.id}
              onClick={runPrimary}
              disabled={action.disabled || anyBusy}
              title={action.reason}
              style={primaryBtnStyle(action.kind, action.disabled || anyBusy)}
            >
              {busyKey(action.kind === "uninstall" ? "remove" : action.kind) ? "Working…" : action.label}
            </button>
          )}
          {showUninstall && (
            <button
              data-testid="extensions-plugin-uninstall"
              data-plugin-id={entry.id}
              onClick={beginRemove}
              disabled={anyBusy}
              style={secondaryBtnStyle(anyBusy)}
            >
              {busyKey("remove") ? "Removing…" : "Uninstall"}
            </button>
          )}
        </div>
      </div>

      {confirmingRemove && (
        <div
          role="dialog"
          aria-label={`Uninstall ${entry.displayName}`}
          data-testid="extensions-uninstall-confirm"
          data-plugin-id={entry.id}
          style={{
            border: "1px solid rgba(229,115,115,0.36)",
            borderRadius: 8,
            padding: 12,
            marginBottom: 14,
            background: "rgba(229,115,115,0.08)",
          }}
        >
          <div style={{ fontSize: 12.5, fontWeight: 600, marginBottom: 8 }}>
            Uninstall {entry.displayName}?
          </div>
          <label
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontSize: 12,
              color: "var(--ct-fg)",
              marginBottom: 10,
            }}
          >
            <input
              type="checkbox"
              data-testid="extensions-uninstall-remove-data"
              checked={removeData}
              onChange={(e) => setRemoveData(e.currentTarget.checked)}
            />
            also remove downloaded data
          </label>
          <div style={{ display: "flex", gap: 8 }}>
            <button
              data-testid="extensions-uninstall-confirm-submit"
              onClick={() => {
                setConfirmingRemove(false);
                onAction("remove", entry, { removeData });
              }}
              disabled={anyBusy}
              style={primaryBtnStyle("uninstall", anyBusy)}
            >
              Uninstall
            </button>
            <button
              data-testid="extensions-uninstall-confirm-cancel"
              onClick={() => setConfirmingRemove(false)}
              disabled={anyBusy}
              style={secondaryBtnStyle(anyBusy)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {entry.pendingReload && (
        <div
          role="status"
          data-testid="extensions-reload-required"
          style={{
            fontSize: 12,
            color: "var(--ct-warn, #f59e0b)",
            background: "var(--ct-warn-bg, rgba(245, 158, 11, 0.12))",
            border: "1px solid var(--ct-warn, #f59e0b)",
            borderRadius: 8,
            padding: "8px 12px",
            marginBottom: 14,
          }}
        >
          Reload required — this plugin is installed but its capability won’t
          activate until you reload CodeTerm.
        </div>
      )}

      {action.reason && action.disabled && (
        <div style={{ fontSize: 11.5, color: "#fca5a5", marginBottom: 14 }} data-testid="extensions-action-disabled-reason">{action.reason}</div>
      )}

      {/* P1: a chatBackend plugin is opened as a non-PTY chat pane from here. */}
      {entry.capabilities?.chat_backend && entry.installed && !entry.pendingReload && (
        <div style={{ marginBottom: 16 }}>
          <button
            data-testid="extensions-open-chat"
            data-plugin-id={entry.id}
            onClick={() => {
              void invoke("open_chat_backend_pane", { pluginId: entry.id }).catch((e) =>
                console.error("open_chat_backend_pane failed:", e),
              );
            }}
            style={primaryBtnStyle("install", false)}
            title={`Open a chat pane backed by ${entry.displayName}`}
          >
            Open chat
          </button>
        </div>
      )}

      {/* Universal "Configure with AI" (Part D): spawns a safety-wrapped pane.
          Shown once installed; works for plugins with or without settings. */}
      {entry.installed && !entry.pendingReload && (
        <div style={{ marginBottom: 16 }}>
          <button
            data-testid="extensions-configure-ai"
            data-plugin-id={entry.id}
            onClick={() => {
              const help =
                entry.configHelp ||
                `Help the user configure and use the ${entry.displayName} plugin.`;
              void (async () => {
                const cfg = await invoke<{ api?: DaemonApiConfig }>("get_config").catch(
                  () => ({}) as { api?: DaemonApiConfig },
                );
                await configureWithAI(entry.id, entry.displayName, help, cfg.api ?? {});
              })();
            }}
            style={primaryBtnStyle("install", false)}
            title={`Open an AI chat that configures ${entry.displayName} for you`}
          >
            Configure with AI
          </button>
        </div>
      )}

      {(capKeys.length > 0 || perms.length > 0) && (
        <div style={{ marginBottom: 18 }} data-testid="extensions-capabilities">
          <GroupLabel>Capabilities</GroupLabel>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 12px" }}>
            {capKeys.map((c) => (
              <CapabilityChip key={`cap-${c}`} variant="system" label={c.replace(/_/g, " ")} title={`Capability: ${c}`} />
            ))}
            {perms.map((c) => (
              <CapabilityChip key={`perm-${c}`} label={capabilityLabel(c)} title={c} />
            ))}
          </div>
        </div>
      )}

      {entry.installed && schema.length > 0 && config && (
        <div style={{ marginBottom: 20 }} data-testid="extensions-settings">
          <GroupLabel>Settings</GroupLabel>
          <SchemaRenderer
            schema={schema}
            providerId={entry.id}
            config={config as unknown as AppConfig}
            values={current}
            onChange={onPluginConfigChange}
            hidePathLinks
          />
        </div>
      )}

      {entry.installed && !entry.pendingReload && supportsPresets && (
        <PresetManager pluginId={entry.id} models={descriptor?.available_models ?? []} />
      )}

      <GroupLabel>About</GroupLabel>
      {entry.readmeMarkdown ? (
        <MarkdownDescription markdown={entry.readmeMarkdown} />
      ) : entry.description ? (
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--ct-fg)", margin: 0 }} data-testid="extensions-description">{entry.description}</p>
      ) : (
        <p style={{ fontSize: 12.5, color: "var(--ct-muted)", margin: 0 }}>
          No description provided for this {catalogKindLabel(entry)}.
        </p>
      )}
      {entry.homepage && (
        <a
          data-testid="extensions-homepage-link"
          href={entry.homepage}
          target="_blank"
          rel="noreferrer noopener"
          style={{ display: "inline-block", marginTop: 10, fontSize: 12, color: "var(--ct-accent, #818cf8)" }}
        >
          {entry.homepage}
        </a>
      )}
    </div>
  );
}

function channelLabel(channel: string): string {
  if (channel === "codeterm-core") return "Core";
  if (channel === "codeterm-plugins") return "CodeTerm Plugins";
  return channel;
}

function primaryBtnStyle(kind: string, disabled: boolean): React.CSSProperties {
  const danger = kind === "uninstall";
  return {
    fontSize: 12.5,
    fontWeight: 560,
    borderRadius: 6,
    padding: "6px 16px",
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.5 : 1,
    border: danger ? "1px solid rgba(229,115,115,0.5)" : "1px solid transparent",
    background: danger ? "transparent" : "var(--ct-accent, #6366f1)",
    color: danger ? "#e57373" : "#fff",
    whiteSpace: "nowrap",
  };
}

function secondaryBtnStyle(disabled: boolean): React.CSSProperties {
  return {
    fontSize: 11.5,
    borderRadius: 6,
    padding: "4px 12px",
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.5 : 1,
    border: "1px solid rgba(229,115,115,0.4)",
    background: "transparent",
    color: "#e57373",
    whiteSpace: "nowrap",
  };
}
