import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { CheckCircle2, Download, RefreshCw, Trash2, Upload, XCircle } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { SettingsCard, SettingsPage, SettingsPill } from "./shared";

type PluginSource =
  | { kind: "dir"; path: string }
  | { kind: "channel"; channel: string; path: string }
  | { kind: "system_channel"; channel: string; path: string };

interface InstalledPlugin {
  id: string;
  version?: string | null;
  display_name?: string | null;
  install_path?: string;
  path?: string;
  install_time?: string | null;
  manifest_hash?: string | null;
  verified?: boolean;
  state?: string;
  source?: PluginSource | null;
  system?: boolean;
  removable?: boolean;
}

interface ChannelRecord {
  name: string;
  url: string;
  path: string;
  kind: "local" | "git";
  system?: boolean;
  removable?: boolean;
}

interface DiffRow {
  channel: string;
  id: string;
  local_version?: string | null;
  remote_version?: string | null;
  drift: string;
  compatible?: boolean;
  blocked_reason?: string | null;
}

interface AuditEvent {
  ts?: string;
  kind?: string;
  plugin?: string;
  channel?: string;
  ok?: boolean;
  error?: string;
  [key: string]: unknown;
}

interface PluginSnapshot {
  plugins: InstalledPlugin[];
  channels: ChannelRecord[];
  diff: DiffRow[];
  audit: AuditEvent[];
  lastOutput?: unknown;
}

const EMPTY: PluginSnapshot = { plugins: [], channels: [], diff: [], audit: [] };

export function PluginsTab() {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [channelUrl, setChannelUrl] = useState("");
  const [channelName, setChannelName] = useState("");

  const load = useCallback(async (clearError = true) => {
    setLoading(true);
    try {
      const next = await invoke<PluginSnapshot>("plugin_manager_snapshot");
      setSnapshot(normalizeSnapshot(next));
      if (clearError) setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const run = useCallback(async (key: string, action: Record<string, unknown>) => {
    setBusyKey(key);
    try {
      const next = await invoke<PluginSnapshot>("plugin_manager_action", { action });
      setSnapshot(normalizeSnapshot(next));
      setError(null);
    } catch (e) {
      setError(String(e));
      await load(false);
    } finally {
      setBusyKey(null);
    }
  }, [load]);

  const diffByPlugin = useMemo(() => {
    const map = new Map<string, DiffRow>();
    for (const row of snapshot.diff) {
      if (row.drift !== "up_to_date") map.set(row.id, row);
    }
    return map;
  }, [snapshot.diff]);

  return (
    <SettingsPage title="Plugins" subtitle="Installed bundles, channels, updates, and audit events.">
      {error && (
        <div
          role="alert"
          data-testid="plugin-manager-error"
          className="rounded-lg border border-[var(--ct-red)]/50 bg-[var(--ct-red)]/10 px-3 py-2 text-xs text-[var(--ct-red)]"
        >
          <span data-testid="plugin-error-toast">{error}</span>
        </div>
      )}

      <SettingsCard
        title="Installed Plugins"
        headerRight={
          <button
            onClick={() => void load()}
            className="w-7 h-7 inline-flex items-center justify-center rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
            title="Refresh"
            data-testid="plugin-refresh"
          >
            <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
          </button>
        }
      >
        <div className="overflow-x-auto">
          <table className="w-full text-xs" data-testid="plugin-installed-list">
            <thead className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
              <tr className="border-b border-[var(--ct-border)]">
                <th className="py-2 pr-3 text-left font-medium">Plugin</th>
                <th className="py-2 pr-3 text-left font-medium">Version</th>
                <th className="py-2 pr-3 text-left font-medium">Source</th>
                <th className="py-2 pr-3 text-left font-medium">State</th>
                <th className="py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {snapshot.plugins.length === 0 && (
                <tr>
                  <td colSpan={5} className="py-4 text-[var(--ct-muted-fg)]">No plugins installed.</td>
                </tr>
              )}
              {snapshot.plugins.map((p) => {
                const diff = diffByPlugin.get(p.id);
                const version = p.version ?? "unknown";
                const updatable = diff?.drift === "update_available";
                const blocked = updatable && diff?.compatible === false;
                const canRemove = p.removable !== false;
                return (
                  <tr
                    key={p.id}
                    data-testid="plugin-row"
                    data-plugin-id={p.id}
                    data-version={version}
                    data-source-kind={p.source?.kind ?? "unknown"}
                    data-source-channel={p.source?.kind === "channel" || p.source?.kind === "system_channel" ? p.source.channel : ""}
                    data-verified={p.verified ? "true" : "false"}
                    data-state={p.state ?? "unknown"}
                    data-update-available={updatable ? "true" : "false"}
                    data-system={p.system ? "true" : "false"}
                    data-removable={canRemove ? "true" : "false"}
                    className="border-b border-[var(--ct-border)]/70"
                  >
                    <td className="py-2 pr-3">
                      <div className="font-medium text-[var(--ct-fg)]">{p.display_name || p.id}</div>
                      <div className="font-mono text-[10px] text-[var(--ct-muted)] truncate max-w-[220px]">{p.id}</div>
                    </td>
                    <td className="py-2 pr-3 font-mono text-[var(--ct-fg)]">
                      <div>{version}</div>
                      {diff?.drift === "update_available" && (
                        <span data-testid="plugin-update-available-badge">
                          <SettingsPill tone="amber">update available</SettingsPill>
                        </span>
                      )}
                    </td>
                    <td className="py-2 pr-3 text-[var(--ct-muted-fg)]">
                      <div>{sourceLabel(p.source)}</div>
                      {p.manifest_hash && <div className="font-mono text-[10px] truncate max-w-[180px]">{p.manifest_hash}</div>}
                    </td>
                    <td className="py-2 pr-3">
                      <div className="flex flex-wrap items-center gap-1.5">
                        {p.system && <SettingsPill tone="accent">system</SettingsPill>}
                        <SettingsPill tone={p.verified ? "green" : "amber"}>{p.verified ? "verified" : "unverified"}</SettingsPill>
                        {p.state && p.state !== "installed" && p.state !== "bundled" && <SettingsPill tone="red">{p.state}</SettingsPill>}
                      </div>
                    </td>
                    <td className="py-2">
                      <div className="flex justify-end items-center gap-1">
                        {blocked ? (
                          <BlockedPill reason={diff?.blocked_reason} />
                        ) : (
                          <IconButton
                            title="Update"
                            testId={`plugin-update-${p.id}`}
                            disabled={busyKey !== null}
                            busy={busyKey === `update-${p.id}`}
                            onClick={() =>
                              void run(
                                `update-${p.id}`,
                                p.system
                                  ? { kind: "update-system", pluginId: p.id }
                                  : { kind: "update", pluginId: p.id },
                              )
                            }
                            icon={<Upload size={14} />}
                          />
                        )}
                        {canRemove && (
                          <IconButton
                            title="Remove"
                            testId={`plugin-remove-${p.id}`}
                            disabled={busyKey !== null}
                            busy={busyKey === `remove-${p.id}`}
                            onClick={() => void run(`remove-${p.id}`, { kind: "remove", pluginId: p.id, force: false })}
                            icon={<Trash2 size={14} />}
                          />
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </SettingsCard>

      <SettingsCard title="Channels">
        <div className="grid grid-cols-[1fr_160px_auto] gap-2">
          <input
            value={channelUrl}
            onChange={(e) => setChannelUrl(e.target.value)}
            placeholder="git or file URL"
            data-testid="plugin-channel-url-input"
            className="min-w-0 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
          />
          <input
            value={channelName}
            onChange={(e) => setChannelName(e.target.value)}
            placeholder="name"
            data-testid="plugin-channel-name-input"
            className="min-w-0 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
          />
          <button
            onClick={() => {
              void run("add-channel", {
                kind: "add-channel",
                url: channelUrl.trim(),
                name: channelName.trim() || undefined,
              });
              setChannelUrl("");
              setChannelName("");
            }}
            disabled={!channelUrl.trim() || busyKey !== null}
            data-testid="plugin-add-channel"
            className="px-3 py-1.5 rounded-md border border-[var(--ct-border)] text-xs text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-50"
          >
            Add
          </button>
        </div>
        <div className="divide-y divide-[var(--ct-border)]/70" data-testid="plugin-channel-list">
          {snapshot.channels.length === 0 && <div className="py-3 text-xs text-[var(--ct-muted-fg)]">No channels registered.</div>}
          {snapshot.channels.map((ch) => (
            <div
              key={ch.name}
              data-testid="plugin-channel"
              data-channel-name={ch.name}
              data-channel-kind={ch.kind}
              data-system={ch.system ? "true" : "false"}
              className="flex items-center justify-between gap-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium text-[var(--ct-fg)]">{ch.name}</span>
                  {ch.system && <SettingsPill tone="accent">system</SettingsPill>}
                </div>
                <div className="text-[10px] text-[var(--ct-muted)] truncate">{ch.url}</div>
              </div>
              <div className="flex gap-1 shrink-0">
                <IconButton
                  title="Refresh channel"
                  testId={`plugin-channel-refresh-${ch.name}`}
                  disabled={busyKey !== null}
                  busy={busyKey === `refresh-${ch.name}`}
                  onClick={() => void run(`refresh-${ch.name}`, { kind: "refresh-channel", channel: ch.name })}
                  icon={<RefreshCw size={14} />}
                />
              </div>
            </div>
          ))}
        </div>
      </SettingsCard>

      <SettingsCard title="Available From Channels">
        <div className="divide-y divide-[var(--ct-border)]/70" data-testid="plugin-diff-list">
          {snapshot.diff.length === 0 && <div className="py-3 text-xs text-[var(--ct-muted-fg)]">No channel diff available.</div>}
          {snapshot.diff.map((row) => (
            <div
              key={`${row.channel}:${row.id}`}
              data-testid="plugin-diff"
              data-plugin-id={row.id}
              data-channel-name={row.channel}
              data-local-version={row.local_version ?? ""}
              data-remote-version={row.remote_version ?? ""}
              data-drift={row.drift}
              data-compatible={row.compatible === false ? "false" : "true"}
              className="flex items-center justify-between gap-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium text-[var(--ct-fg)]">{row.id}</span>
                  <SettingsPill tone={row.drift === "up_to_date" ? "green" : row.drift === "not_installed" ? "accent" : "amber"}>
                    {row.drift.replaceAll("_", " ")}
                  </SettingsPill>
                </div>
                <div className="text-[10px] text-[var(--ct-muted)]">
                  {row.channel}: {row.local_version ?? "not installed"} {"->"} {row.remote_version ?? "missing"}
                </div>
              </div>
              <div className="shrink-0">
                {row.compatible === false && row.drift !== "up_to_date" ? (
                  <BlockedPill reason={row.blocked_reason} />
                ) : row.drift === "not_installed" ? (
                  <IconButton
                    title="Install"
                    testId={`plugin-install-${row.id}`}
                    disabled={busyKey !== null}
                    busy={busyKey === `install-${row.id}`}
                    onClick={() => void run(`install-${row.id}`, { kind: "install", pluginId: row.id, channel: row.channel })}
                    icon={<Download size={14} />}
                  />
                ) : row.drift !== "up_to_date" ? (
                  <IconButton
                    title="Update"
                    testId={`plugin-diff-update-${row.id}`}
                    disabled={busyKey !== null}
                    busy={busyKey === `update-${row.id}`}
                    onClick={() => void run(`update-${row.id}`, { kind: "update", pluginId: row.id })}
                    icon={<Upload size={14} />}
                  />
                ) : (
                  <CheckCircle2 size={15} className="text-[var(--ct-green)]" />
                )}
              </div>
            </div>
          ))}
        </div>
      </SettingsCard>

      <SettingsCard title="Recent Events">
        <div className="max-h-64 overflow-y-auto divide-y divide-[var(--ct-border)]/70" data-testid="plugin-audit-log">
          {snapshot.audit.length === 0 && <div className="py-3 text-xs text-[var(--ct-muted-fg)]">No plugin events recorded.</div>}
          {snapshot.audit.map((ev, index) => (
            <div
              key={index}
              data-testid={ev.ok === false ? "plugin-error-toast" : "plugin-audit-event"}
              data-event-kind={ev.kind ?? "event"}
              data-ok={ev.ok === false ? "false" : "true"}
              className="grid grid-cols-[18px_140px_1fr] gap-2 py-2 text-xs"
            >
              {ev.ok === false ? <XCircle size={14} className="text-[var(--ct-red)] mt-0.5" /> : <CheckCircle2 size={14} className="text-[var(--ct-green)] mt-0.5" />}
              <div className="font-mono text-[10px] text-[var(--ct-muted)] truncate">{formatTime(ev.ts)}</div>
              <div className="min-w-0">
                <div className="text-[var(--ct-fg)]">{ev.kind ?? "event"} {ev.plugin ? `- ${ev.plugin}` : ""}</div>
                <div className="text-[10px] text-[var(--ct-muted)] truncate">
                  {ev.channel ? `${ev.channel} ` : ""}{ev.error ?? ""}
                </div>
              </div>
            </div>
          ))}
        </div>
      </SettingsCard>
    </SettingsPage>
  );
}

function normalizeSnapshot(value: PluginSnapshot | null | undefined): PluginSnapshot {
  return {
    plugins: value?.plugins ?? [],
    channels: value?.channels ?? [],
    diff: value?.diff ?? [],
    audit: value?.audit ?? [],
    lastOutput: value?.lastOutput,
  };
}

function sourceLabel(source: PluginSource | null | undefined): string {
  if (!source) return "unknown";
  if (source.kind === "dir") return "local directory";
  return source.channel;
}

function formatTime(value: string | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function BlockedPill({ reason }: { reason?: string | null }) {
  return (
    <span title={reason ?? undefined} data-testid="plugin-update-blocked">
      <SettingsPill tone="red">incompatible</SettingsPill>
    </span>
  );
}

function IconButton({
  title,
  testId,
  icon,
  onClick,
  disabled,
  busy,
}: {
  title: string;
  testId: string;
  icon: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  busy?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      data-testid={testId}
      className="w-7 h-7 inline-flex items-center justify-center rounded-md border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-50"
    >
      {busy ? <RefreshCw size={14} className="animate-spin" /> : icon}
    </button>
  );
}
