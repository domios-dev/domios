import { InlineErrorNotice } from "./shared/ErrorNotice";
import { Download } from "lucide-react";
import { useUpdatePanel } from "../hooks/useUpdatePanel";
import { groupUpdates } from "../lib/updatePanelModel";
import { useEffect } from "react";
import { ensurePluginSnapshot, usePluginSnapshotStore } from "../lib/pluginSnapshotStore";
import { ComponentStatus } from "./shared/ComponentStatus";
import { UpdateChannelNotice } from "./shared/UpdateChannelNotice";
import { SettingsCard, SettingsPill } from "./Settings/shared";
import { BusySpinner } from "./shared/BusyState";
import { Button } from "./shared/Button";

export function UpdatePanelStatus() {
  const update = useUpdatePanel();
  const available = update.rows.some(row => !row.channelError && row.state === "available");
  const completed = update.rows.some(row => row.state === "completed");
  const failed = Boolean(update.error) || update.rows.some(row => row.state === "failed") || update.diagnostics.some(item => item.channel !== "codeterm-core");
  return <span role="status" aria-label={failed ? "Update sources or actions failed" : available ? "Updates available" : update.busy ? "Updating components" : "No applicable updates"}><SettingsPill tone={failed ? "red" : update.busy || available ? "accent" : completed ? "green" : "muted"}>
    {update.busy ? "updating" : failed ? "failed" : available ? "updates available" : completed ? "updated" : "up to date"}
  </SettingsPill></span>;
}

export function UpdatePanel() {
  return <SettingsCard icon={<Download size={16} />} title="Updates" headerRight={<UpdatePanelStatus />}>
    <UpdatePanelContents />
  </SettingsCard>;
}

export function UpdatePanelContents() {
  const update = useUpdatePanel();
  const { snapshot } = usePluginSnapshotStore();
  useEffect(() => { void ensurePluginSnapshot({ background: true }).catch(() => undefined); }, []);
  return (
      <div className="flex flex-col gap-3">
        {groupUpdates(update.rows).map(section => (
          <section key={section.group} aria-label={`${section.group} updates`}>
            <h3 className="mb-1 text-xs font-semibold text-[var(--ct-muted)]">{section.group}</h3>
            <div className="min-w-0">
              {section.items.map(item => {
                const row = update.rows.find(row => row.id === item.id)!;
                const metadata = snapshot.plugins.find(plugin => plugin.id === item.id) ?? snapshot.marketplace?.find(plugin => plugin.id === item.id);
                const description = metadata?.pitch || metadata?.description || (item.group === "App" ? "Desktop app" : item.group === "Interface" ? "Web interface" : "Plugin");
                const canApply = !item.channelError && update.items.some(current => current.id === item.id)
                  && (row.state === "available" || (row.state === "failed" && !update.error && !update.awaitingRetry));
                return <div key={item.id} className="grid grid-cols-[minmax(0,1fr)_auto_14px_44px] items-center gap-x-2 gap-y-1 border-b border-[var(--ct-border)] py-2 last:border-0">
                  <div className="flex min-w-0 items-baseline gap-2" title={description}>
                    <span className="max-w-[40%] shrink-0 truncate text-xs font-medium text-[var(--ct-fg)]">{item.name}</span>
                    <span className="min-w-0 truncate text-[11px] text-[var(--ct-muted-fg)]">{description}</span>
                  </div>
                  <span title={`${item.currentVersion} → ${item.targetVersion}`} className="max-w-[120px] truncate text-[11px] tabular-nums text-[var(--ct-muted-fg)]">{item.currentVersion} → {item.targetVersion}</span>
                  <ComponentStatus state={row.state} label={`${item.name} update ${row.state}`} />
                  <div>{canApply && <Button size="sm" disabled={update.busy || update.checking} aria-label={`Apply ${item.name} update`} onClick={() => void update.applyItem(item.id)}>Apply</Button>}</div>
                  {row.state === "active" && <progress aria-label={`${item.name} update progress`} max={100} value={item.group === "App" ? update.appProgress ?? undefined : undefined} className="col-span-4 h-1 w-full accent-[var(--ct-accent)]" />}
                </div>;
              })}
            </div>
          </section>
        ))}
        {(update.checking || update.checkResult) && <div role="status" aria-label="Update check result">{update.checking ? "Checking for updates..." : update.checkResult}</div>}
        {update.error && <InlineErrorNotice description={{ title: `${update.rows.filter(row => row.state === "failed").map(row => row.name).join(", ") || "Update"} could not complete`, cause: `${update.error} Check again, then apply the available update.`, actions: ["retry"], raw: update.errorDetails ?? update.error }} blocking />}
        {update.history.length > 0 && <details><summary>Previous update failures</summary>{update.history.map((failure, index) => <InlineErrorNotice key={index} description={{ title: "Previous update failed", cause: failure.message, raw: failure.details, actions: [] }} />)}</details>}
        {update.information.map(message => <div role="status" aria-label="Installed plugin ahead of channel" key={message} className="text-xs text-[var(--ct-muted)]">{message}</div>)}
        {update.diagnostics.map(({ channel, message }) => <div key={channel} aria-label={`${channel} channel diagnostics`}><UpdateChannelNotice channel={channel} message={message} /></div>)}
        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--ct-border)] pt-3">
          <Button size="sm" variant="accent" aria-label="Update all" disabled={update.busy || update.checking || update.awaitingRetry || !update.rows.some(item => !item.channelError && item.state !== "completed" && update.items.some(offer => offer.id === item.id))} onClick={() => void update.updateAll()}><span className="inline-flex items-center gap-2">{update.busy && <BusySpinner size={14} />}{update.busy ? "Updating components" : "Update all"}</span></Button>
          <Button size="sm" disabled={update.checking || (update.busy && !update.error)} onClick={() => void update.retry()}>Check again</Button>
        </div>
      </div>
  );
}
