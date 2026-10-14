import { Download } from "lucide-react";
import { useUpdatePanel } from "../hooks/useUpdatePanel";
import { groupUpdates } from "../lib/updatePanelModel";
import { SettingsCard, SettingsPill } from "./Settings/shared";
import { Button } from "./shared/Button";

export function UpdatePanel() {
  const update = useUpdatePanel();
  return (
    <SettingsCard icon={<Download size={16} />} title="Updates"
      headerRight={<SettingsPill tone={update.error ? "red" : update.items.length ? "accent" : "muted"}>{update.error ? "failed" : update.busy ? "updating" : update.items.length ? "updates available" : "up to date"}</SettingsPill>}>
      <UpdatePanelContents />
    </SettingsCard>
  );
}

export function UpdatePanelContents() {
  const update = useUpdatePanel();
  return (
      <div className="flex flex-col gap-3">
        {groupUpdates(update.items).map(section => (
          <section key={section.group} aria-label={`${section.group} updates`}>
            <h3 className="mb-1 text-xs font-semibold text-[var(--ct-muted)]">{section.group}</h3>
            {section.items.map(item => <div key={item.id} className="flex items-center justify-between gap-3 py-1 text-xs">
              <span>{item.name}</span>
              <span className="font-mono text-[var(--ct-muted)]">{item.currentVersion} → {item.targetVersion}</span>
              <Button size="sm" disabled={update.busy} aria-label={`Apply ${item.name} update`} onClick={() => void update.applyItem(item.id)}>Apply</Button>
            </div>)}
          </section>
        ))}
        {update.error && <div role="alert" className="text-xs text-[var(--ct-red)]">{update.error}</div>}
        {update.information.map(message => <div role="status" aria-label="Installed plugin ahead of channel" key={message} className="text-xs text-[var(--ct-muted)]">{message}</div>)}
        <div className="flex gap-2">
          <Button size="sm" variant="accent" disabled={update.busy || !update.items.length} onClick={() => void update.updateAll()} loading={update.busy}>Update all</Button>
          {update.error && <Button size="sm" onClick={() => void update.retry()}>Retry</Button>}
        </div>
      </div>
  );
}
