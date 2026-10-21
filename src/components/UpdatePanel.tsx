import { InlineErrorNotice } from "./shared/ErrorNotice";
import { CheckCircle2, CircleAlert, Download } from "lucide-react";
import { useUpdatePanel } from "../hooks/useUpdatePanel";
import { groupUpdates } from "../lib/updatePanelModel";
import { CORE_PLUGIN_CHANNEL } from "../lib/updateStatusModel";
import { SettingsCard, SettingsPill } from "./Settings/shared";
import { BusySpinner } from "./shared/BusyState";
import { Button } from "./shared/Button";

export function UpdatePanelStatus() {
  const update = useUpdatePanel();
  const available = update.rows.some(row => row.state === "available");
  const completed = update.rows.some(row => row.state === "completed");
  const failed = Boolean(update.error) || update.rows.some(row => row.state === "failed");
  return <SettingsPill tone={failed ? "red" : update.busy || available ? "accent" : completed ? "green" : "muted"}>
    {update.busy ? "updating" : failed ? "failed" : available ? "updates available" : completed ? "updated" : "up to date"}
  </SettingsPill>;
}

export function UpdatePanel() {
  return <SettingsCard icon={<Download size={16} />} title="Updates" headerRight={<UpdatePanelStatus />}>
    <UpdatePanelContents />
  </SettingsCard>;
}

export function UpdatePanelContents() {
  const update = useUpdatePanel();
  return (
      <div className="flex flex-col gap-3">
        {groupUpdates(update.rows).map(section => (
          <section key={section.group} aria-label={`${section.group} updates`}>
            <h3 className="mb-1 text-xs font-semibold text-[var(--ct-muted)]">{section.group}</h3>
            <div className="divide-y divide-[var(--ct-border)]">
              {section.items.map(item => {
                const row = update.rows.find(row => row.id === item.id)!;
                return <div key={item.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2 py-3">
                  <div className="min-w-0">
                    <div className="break-words text-sm font-medium text-[var(--ct-fg)]">{item.name}</div>
                    <div className="mt-1 break-words text-xs tabular-nums text-[var(--ct-muted-fg)]">{item.currentVersion} → {item.targetVersion}</div>
                  </div>
                  {(row.state === "available" || Boolean(item.channelError)) ? <Button size="sm" disabled={update.busy || update.checking || Boolean(item.channelError)} aria-label={`Apply ${item.name} update`} onClick={() => void update.applyItem(item.id)}>Apply</Button>
                    : <div role="status" aria-label={`${item.name} update ${row.state}`} className={`flex items-center gap-2 text-xs ${row.state === "failed" ? "text-[var(--ct-red)]" : row.state === "completed" ? "text-[var(--ct-green)]" : "text-[var(--ct-muted-fg)]"}`}>
                      {row.state === "active" ? <BusySpinner size={14} /> : row.state === "completed" ? <CheckCircle2 size={14} /> : row.state === "failed" ? <CircleAlert size={14} /> : null}
                      <span>{row.state === "active" ? "Updating" : row.state === "queued" ? "Queued" : row.state === "completed" ? row.alreadyCurrent ? "Already up to date" : "Updated" : "Failed"}</span>
                    </div>}
                  {row.state === "active" && <progress aria-label={`${item.name} update progress`} max={100} value={item.group === "App" ? update.appProgress ?? undefined : undefined} className="col-span-2 h-1.5 w-full accent-[var(--ct-accent)]" />}
                  {item.channelError && <div className="col-span-2"><InlineErrorNotice description={{ title: `${item.name} plugin updates unavailable`, cause: item.channelError, actions: [], raw: "" }} /></div>}
                </div>;
              })}
            </div>
          </section>
        ))}
        {(update.checking || update.checkResult) && <div role="status" aria-label="Update check result">{update.checking ? "Checking for updates..." : update.checkResult}</div>}
        {update.error && <InlineErrorNotice description={{ title: "Update could not complete", cause: update.errorDetails ?? update.error, actions: ["retry"], raw: update.errorDetails ?? update.error }} blocking />}
        {update.information.map(message => <div role="status" aria-label="Installed plugin ahead of channel" key={message} className="text-xs text-[var(--ct-muted)]">{message}</div>)}
        {update.diagnostics.map(({ channel, message }) => <div role="status" aria-label={`${channel} channel diagnostics`} key={channel} className="text-xs text-[var(--ct-muted)]">
          {channel === CORE_PLUGIN_CHANNEL ? `Core plugins: ${message} Bundled core plugins stay installed.` : `${channel}: ${message}`}
        </div>)}
        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--ct-border)] pt-3">
          <Button size="sm" variant="accent" aria-label="Update all" disabled={update.busy || update.checking || update.awaitingRetry || !update.rows.some(item => !item.channelError && item.state !== "completed")} onClick={() => void update.updateAll()}><span className="inline-flex items-center gap-2">{update.busy && <BusySpinner size={14} />}{update.busy ? "Updating components" : "Update all"}</span></Button>
          <Button size="sm" disabled={update.checking || (update.busy && !update.error)} onClick={() => void update.retry()}>Check again</Button>
        </div>
      </div>
  );
}
