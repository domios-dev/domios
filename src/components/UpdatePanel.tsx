import { Download } from "lucide-react";
import { useUpdatePanel } from "../hooks/useUpdatePanel";
import { groupUpdates } from "../lib/updatePanelModel";
import { SettingsCard, SettingsPill } from "./Settings/shared";
import { Button } from "./shared/Button";

export function UpdatePanel() {
  const update = useUpdatePanel();
  return (
    <SettingsCard icon={<Download size={16} />} title="Updates"
      headerRight={<SettingsPill tone={update.items.length ? "accent" : "muted"}>{update.busy ? "updating" : update.items.length ? "updates available" : "up to date"}</SettingsPill>}>
      <div className="flex flex-col gap-3">
        {groupUpdates(update.items).map(section => (
          <section key={section.group} aria-label={`${section.group} updates`}>
            <h3 className="mb-1 text-xs font-semibold text-[var(--ct-muted)]">{section.group}</h3>
            {section.items.map(item => <div key={item.id} className="flex items-center justify-between gap-3 py-1 text-xs">
              <span>{item.name}</span>
              <span className="font-mono text-[var(--ct-muted)]">{item.currentVersion} → {item.targetVersion}</span>
            </div>)}
          </section>
        ))}
        {update.error && <div role="alert" className="text-xs text-[var(--ct-red)]">{update.error}</div>}
        {Object.entries(update.snapshot.errors).map(([source, error]) => <div role="status" key={source} className="text-xs text-[var(--ct-muted)]">{source}: {error}</div>)}
        <div className="flex gap-2">
          <Button size="sm" variant="accent" disabled={update.busy || !update.items.length} onClick={() => void update.updateAll()} loading={update.busy}>Update all</Button>
          {update.error && <Button size="sm" onClick={() => void update.retry()}>Retry</Button>}
        </div>
      </div>
    </SettingsCard>
  );
}
