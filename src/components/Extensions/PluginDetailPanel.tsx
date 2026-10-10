import { useState } from "react";
import {
  capabilityKeys,
  catalogKindLabel,
  primaryAction,
  type CatalogEntry,
} from "./pluginCatalog";
import { CapabilityChip, capabilityLabel } from "./CapabilityChip";
import { SchemaRenderer } from "../Settings/SchemaRenderer";
import { PresetManager } from "./PresetManager";
import { ChatPreprocessorPresetManager } from "./ChatPreprocessorPresetManager";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { AgentProviderScreen } from "../Settings/AgentsTab";
import { SecretBackendDetail } from "./SecretBackendDetail";
import { PluginView } from "./PluginView";
import { MarkdownDescription } from "./MarkdownDescription";
import { GroupLabel, type AppConfigShape } from "./shared";
import { invoke } from "../../lib/bridge";
import type { AppConfig, PluginActivity, ProviderConfig } from "../../lib/types";
import type { PluginActivityDisplayMap } from "../../lib/usePluginActivity";
import { Button } from "../shared/Button";
import { PluginDetailHeader } from "./PluginDetailHeader";

export type ActionKind = "install" | "update" | "remove" | "settings";
export type ActionOptions = { removeData?: boolean; allowHosts?: string[]; disallowHosts?: string[] };

export function PluginDetailPanel({
  entry,
  config,
  busy,
  onAction,
  onHostGrant,
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
  onHostGrant: (pluginId: string, host: string, allow: boolean) => void;
  onSave: (pluginId: string, key: string, value: unknown) => Promise<void>;
  onUpdateFullConfig: (next: AppConfig) => void | Promise<void>;
  onGone: () => void;
}) {
  // Keys off the transport-tagged descriptor, never a provider id.
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

  // installed; before install, fall through to the generic install view).
  if (entry.capabilities?.secret_backend && entry.installed && config) {
    return <SecretBackendDetail entry={entry} config={config} onAction={onAction} actionBusy={busy} activity={activity} activityDisplayMap={activityDisplayMap} />;
  }

  const action = primaryAction(entry);

  const schema = entry.settingsSchema ?? [];
  const current = (config?.plugins?.[entry.id] ?? {}) as Record<string, unknown>;
  const perms = entry.grantedPermissions ?? [];
  const capKeys = capabilityKeys(entry.capabilities);

  const onPluginConfigChange = (id: string, patch: Partial<ProviderConfig>) => {
    for (const [k, v] of Object.entries(patch)) void onSave(id, k, v);
  };

  return (
    <div data-testid="extensions-plugin-detail" data-plugin-id={entry.id}>
      <PluginDetailHeader
        entry={entry}
        busy={busy}
        onAction={onAction}
        activity={activity}
        activityDisplayMap={activityDisplayMap}
      />

      {entry.pendingReload && (
        <div
          role="status"
          data-testid="extensions-reload-required"
          style={{
            fontSize: 12,
            color: "var(--ct-warn)",
            background: "color-mix(in srgb, var(--ct-yellow) 12%, transparent)",
            border: "1px solid var(--ct-warn)",
            borderRadius: 8,
            padding: "8px 12px",
            marginBottom: 14,
          }}
        >
          Reload Domios to activate this plugin.
        </div>
      )}

      {entry.loadError && (
        <div
          role="alert"
          data-testid="extensions-load-error"
          style={{
            fontSize: 12,
            color: "var(--ct-red)",
            background: "color-mix(in srgb, var(--ct-red) 10%, transparent)",
            border: "1px solid var(--ct-red)",
            borderRadius: 8,
            padding: "8px 12px",
            marginBottom: 14,
            overflowWrap: "anywhere",
          }}
        >
          <div style={{ fontWeight: 600, marginBottom: 4 }}>
            {entry.loadError.keptPrevious ? "Last reload failed; the previous version keeps running." : "This plugin failed to load."}
          </div>
          <div>{entry.loadError.message}</div>
          <div style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 10.5, opacity: 0.8, marginTop: 4 }}>{entry.loadError.path}</div>
        </div>
      )}

      {action.reason && action.disabled && (
        <div style={{ fontSize: 11.5, color: "var(--ct-red)", marginBottom: 14 }} data-testid="extensions-action-disabled-reason">{action.reason}</div>
      )}

      {entry.capabilities?.chat_backend && entry.installed && !entry.pendingReload && (
        <div style={{ marginBottom: 16 }}>
          <Button
            variant="primary"
            size="sm"
            data-testid="extensions-open-chat"
            data-plugin-id={entry.id}
            onClick={() => {
              void invoke("open_chat_backend_tab", { pluginId: entry.id }).catch((e) =>
                console.error("open_chat_backend_tab failed:", e),
              );
            }}
            title={`Open a chat tab backed by ${entry.displayName}`}
          >
            Open chat
          </Button>
        </div>
      )}

      {entry.capabilities?.view && entry.installed && !entry.pendingReload && config && (
        <section aria-label={`${entry.displayName} setup`} style={{ marginBottom: 18 }} data-testid="extensions-plugin-view" data-plugin-id={entry.id}>
          <PluginView pluginId={entry.id} api={config.api} embedded />
        </section>
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

      {entry.installed && (
        <AllowedHostsEditor
          entry={entry}
          baseUrl={typeof current.baseUrl === "string" ? current.baseUrl : ""}
          busy={busy === `settings:${entry.id}`}
          onChange={(host, allow) => onHostGrant(entry.id, host, allow)}
        />
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

      {entry.installed && !entry.pendingReload && entry.capabilities?.chat_preprocessor && (
        <ChatPreprocessorPresetManager pluginId={entry.id} />
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

function normalizeNetworkHost(value: string): string | null {
  const candidate = value.trim();
  if (!candidate || candidate !== value || /[\s*\/?#@%]|:\/\//.test(candidate)) return null;
  let host: string;
  let port = "";
  if (candidate.startsWith("[")) {
    const end = candidate.indexOf("]");
    if (end < 0) return null;
    host = candidate.slice(0, end + 1);
    const rest = candidate.slice(end + 1);
    if (rest && !rest.startsWith(":")) return null;
    port = rest.slice(1);
  } else {
    const separator = candidate.lastIndexOf(":");
    if (separator >= 0) {
      if (candidate.indexOf(":") !== separator) return null;
      host = candidate.slice(0, separator);
      port = candidate.slice(separator + 1);
    } else {
      host = candidate;
    }
  }
  if (candidate.endsWith(":") || (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535))) return null;
  try {
    const parsed = new URL(`http://${host}`);
    const hostname = parsed.hostname.toLowerCase().replace(/\.$/, "");
    if (!hostname || parsed.username || parsed.password || parsed.pathname !== "/") return null;
    if (/[\u0080-\uffff]/.test(host)) return null;
    if (!hostname.startsWith("[") && (
      hostname.length > 253
      || !hostname.split(".").every((label) =>
        label.length > 0
        && label.length <= 63
        && /^[a-z0-9]/.test(label)
        && /[a-z0-9]$/.test(label)
        && /^[a-z0-9-]+$/.test(label),
      )
    )) return null;
    const normalizedHost = host.startsWith("[")
      ? hostname
      : host.toLowerCase().replace(/\.$/, "");
    return port ? `${normalizedHost}:${Number(port)}` : normalizedHost;
  } catch {
    return null;
  }
}

function baseUrlHost(value: string): string | null {
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i.exec(value)?.[1];
  return authority ? normalizeNetworkHost(authority) : null;
}

function hostMatches(allowedValue: string, requestedValue: string): boolean {
  const allowed = normalizeNetworkHost(allowedValue);
  const requested = normalizeNetworkHost(requestedValue);
  if (!allowed || !requested) return false;
  const split = (authority: string) => {
    if (authority.startsWith("[")) {
      const end = authority.indexOf("]");
      const host = authority.slice(0, end + 1);
      const suffix = authority.slice(end + 1);
      return [host, suffix.slice(1)] as const;
    }
    const colon = authority.lastIndexOf(":");
    return colon < 0 ? [authority, ""] as const : [authority.slice(0, colon), authority.slice(colon + 1)] as const;
  };
  const [allowedHost, allowedPort] = split(allowed);
  const [requestedHost, requestedPort] = split(requested);
  const allowedHasPort = allowedPort.length > 0;
  const requestedHasPort = requestedPort.length > 0;
  return allowedHost === requestedHost && (!allowedHasPort || (requestedHasPort && allowedPort === requestedPort));
}

function AllowedHostsEditor({
  entry,
  baseUrl,
  busy,
  onChange,
}: {
  entry: CatalogEntry;
  baseUrl: string;
  busy: boolean;
  onChange: (host: string, allow: boolean) => void;
}) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const allowed = entry.networkAllow ?? [];
  const effective = entry.effectiveNetworkAllow ?? [];
  const configuredHost = baseUrlHost(baseUrl);
  const missingBaseHost = configuredHost && !effective.some((host) => hostMatches(host, configuredHost))
    ? configuredHost
    : null;
  const normalizedDraft = normalizeNetworkHost(draft);

  const add = () => {
    if (!normalizedDraft) {
      setError("Enter an exact hostname or IP, with an optional port. Schemes and wildcards are not allowed.");
      return;
    }
    if (allowed.some((host) => hostMatches(host, normalizedDraft) && hostMatches(normalizedDraft, host))) {
      setError(`${normalizedDraft} is already allowed.`);
      return;
    }
    setError(null);
    onChange(normalizedDraft, true);
    setDraft("");
  };

  return (
    <section aria-label="Allowed hosts" data-testid="extensions-allowed-hosts" style={{ marginBottom: 18 }}>
      <GroupLabel>Allowed hosts</GroupLabel>
      <p style={{ fontSize: 11.5, color: "var(--ct-muted)", margin: "0 0 8px" }}>
        Exact hosts only; add :port to limit one port. Applies the next time the plugin loads.
      </p>
      {missingBaseHost && (
        <div role="status" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 8 }}>
          <span>Configured base URL host {missingBaseHost} is not allowed.</span>
          <Button variant="accent" size="sm" aria-label={`Allow ${missingBaseHost}`} onClick={() => onChange(missingBaseHost, true)} disabled={busy}>
            Allow {missingBaseHost}
          </Button>
        </div>
      )}
      <ul aria-label="Current allowed hosts" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 5 }}>
        {allowed.map((host) => (
          <li key={host} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
            <code>{host}</code>
            <Button variant="ghost" size="sm" aria-label={`Remove ${host}`} onClick={() => onChange(host, false)} disabled={busy}>
              Remove
            </Button>
          </li>
        ))}
        {allowed.length === 0 && <li style={{ fontSize: 12, color: "var(--ct-muted)" }}>No user-granted hosts.</li>}
      </ul>
      <div style={{ display: "flex", gap: 8, marginTop: 9 }}>
        <label htmlFor={`allowed-host-${entry.id}`} className="sr-only">Host or host:port</label>
        <input
          id={`allowed-host-${entry.id}`}
          aria-label="Host or host:port"
          className="ct-input ct-input--compact ct-input--mono ct-text-mono min-w-0 flex-1"
          value={draft}
          onChange={(event) => { setDraft(event.currentTarget.value); setError(null); }}
          onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); add(); } }}
          placeholder="api.example.com:8443"
          autoComplete="off"
          disabled={busy}
        />
        <Button variant="accent" size="sm" aria-label="Allow host" onClick={add} disabled={busy || !draft.trim()}>
          Allow host
        </Button>
      </div>
      {error && <p role="alert" aria-label="Allowed host validation error" style={{ fontSize: 11.5, color: "var(--ct-red)", margin: "6px 0 0" }}>{error}</p>}
    </section>
  );
}
