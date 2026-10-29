import { InlineErrorNotice } from "./shared/ErrorNotice";
import { Download, RotateCw } from "lucide-react";
import { useUpdatePanel } from "../hooks/useUpdatePanel";
import { groupUpdates, summarizeUpdatePanel, updateSourceMessage, type UpdateRow } from "../lib/updatePanelModel";
import { ComponentStatus } from "./shared/ComponentStatus";
import { UpdateChannelNotice } from "./shared/UpdateChannelNotice";
import { SettingsCard } from "./Settings/shared";
import type { ReactNode } from "react";
import { Button } from "./shared/Button";
import { BrandMark } from "./brand/BrandMark";
import { DotMatrixCanvas } from "./brand/DotMatrixCanvas";
import { useBusyWaves } from "./brand/useBusyWaves";
import { usePrefersReducedMotion } from "./brand/usePrefersReducedMotion";

const STATE_TEXT: Record<UpdateRow["state"], string> = {
  available: "Available",
  active: "Updating",
  queued: "Queued",
  completed: "Updated",
  failed: "Failed",
};

const STATE_TONE: Record<UpdateRow["state"], string> = {
  available: "text-[var(--ct-accent)]",
  active: "text-[var(--ct-accent)]",
  queued: "text-[var(--ct-muted-fg)]",
  completed: "text-[var(--ct-green)]",
  failed: "text-[var(--ct-red)]",
};

function UpdatePanelSummaryLine() {
  const update = useUpdatePanel();
  const failed = Boolean(update.error) || update.rows.some(row => row.state === "failed") || update.diagnostics.some(item => item.channel !== "codeterm-core");
  const summary = summarizeUpdatePanel(update.rows, { busy: update.busy, failed });
  return <span role="status" aria-label={summary.label} className={summary.tone === "red" ? "text-[var(--ct-red)]" : undefined}>{summary.sentence}</span>;
}

/** The one Updates card: the titlebar popover and Settings both render it; `waves` off where the host page owns the dot stage. */
export function UpdatePanel({ children, waves = true }: { children?: ReactNode; waves?: boolean }) {
  const { busy, error } = useUpdatePanel();
  const reducedMotion = usePrefersReducedMotion();
  const { matrix, emitWave } = useBusyWaves(busy, Boolean(error));
  return <SettingsCard
    backdrop={waves ? <DotMatrixCanvas ref={matrix} mode="loading" reducedMotion={reducedMotion} intensity={0.35} /> : undefined}
    icon={waves && busy ? <span data-dot-origin className="inline-flex"><BrandMark motion="load" onRelease={emitWave} /></span> : <Download size={16} />}
    title="Updates" description={<UpdatePanelSummaryLine />}>
    <UpdatePanelContents>{children}</UpdatePanelContents>
  </SettingsCard>;
}

function RowState({ row }: { row: UpdateRow }) {
  return <span className={`inline-flex items-center gap-1.5 text-[11px] font-medium ${STATE_TONE[row.state]}`}>
    <ComponentStatus state={row.state} label={`${row.name} update ${row.state}`} />
    <span aria-hidden="true">{row.channelError ? "Unavailable" : STATE_TEXT[row.state]}</span>
  </span>;
}

function UpdatePanelContents({ children }: { children?: ReactNode }) {
  const update = useUpdatePanel();
  return (
      <div className="flex flex-col gap-3">
        {groupUpdates(update.rows).map(section => (
          <section key={section.group} aria-label={`${section.group} updates`}>
            <h3 className="mb-0.5 text-[11px] font-medium text-[var(--ct-muted)]">{section.group}</h3>
            <div className="min-w-0">
              {section.items.map(item => {
                const row = update.rows.find(row => row.id === item.id)!;
                const canApply = !item.channelError && update.items.some(current => current.id === item.id)
                  && (row.state === "available" || (row.state === "failed" && !update.error && !update.awaitingRetry));
                return <div key={item.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1.5 border-b border-[var(--ct-border)] py-2 last:border-0">
                  <div className="flex min-w-0 items-baseline gap-2">
                    <span className="min-w-0 truncate text-xs font-medium text-[var(--ct-fg)]">{item.name}</span>
                    <span title={`${item.currentVersion} → ${item.targetVersion}`} className="shrink-0 text-[11px] tabular-nums text-[var(--ct-muted)]">
                      {item.currentVersion} → <span className="text-[var(--ct-muted-fg)]">{item.targetVersion}</span>
                    </span>
                  </div>
                  <div className="flex items-center justify-end gap-2">
                    {(row.state !== "available" || !canApply) && <RowState row={row} />}
                    {canApply && <Button variant="accent" size="compact" disabled={update.busy || update.checking} aria-label={`Update ${item.name}`} onClick={() => void update.applyItem(item.id)}>Update</Button>}
                  </div>
                  {row.state === "active" && <progress aria-label={`${item.name} update progress`} max={100} value={item.group === "App" ? update.appProgress ?? undefined : undefined} className="ct-progress col-span-2" />}
                  {((row.channelError && !row.channel) || row.sourceWarning) && <div className="col-span-2"><InlineErrorNotice description={{
                    title: row.sourceWarning ? "Cached update remains available" : `${item.name} updates unavailable`,
                    cause: updateSourceMessage(row.channelError ?? row.sourceWarning!),
                    raw: row.channelError ?? row.sourceWarning!,
                    actions: [],
                  }} /></div>}
                </div>;
              })}
            </div>
          </section>
        ))}
        {(update.checking || update.checkResult) && <div role="status" aria-label="Update check result" className="text-[11px] text-[var(--ct-muted-fg)]">{update.checking ? "Checking for updates..." : update.checkResult}</div>}
        {update.error && <InlineErrorNotice description={{ title: `${update.rows.filter(row => row.state === "failed").map(row => row.name).join(", ") || "Update"} could not complete`, cause: `${update.error} Check again, then apply the available update.`, actions: ["retry"], raw: update.errorDetails ?? update.error }} blocking />}
        {update.history.length > 0 && <details className="text-[11px] text-[var(--ct-muted-fg)]"><summary className="cursor-pointer hover:text-[var(--ct-fg)]">Previous update failures</summary>{update.history.map((failure, index) => <InlineErrorNotice key={index} description={{ title: "Previous update failed", cause: failure.message, raw: failure.details, actions: [] }} />)}</details>}
        {update.information.map(message => <div role="status" aria-label="Installed plugin ahead of channel" key={message} className="text-[11px] text-[var(--ct-muted-fg)]">{message}</div>)}
        {update.diagnostics.map(({ channel, message }) => <div key={channel} aria-label={`${channel} channel diagnostics`}><UpdateChannelNotice channel={channel} message={message} /></div>)}
        {children}
        <div className="flex items-center justify-between gap-2 border-t border-[var(--ct-border)] pt-3">
          <Button variant="ghost" size="compact" className="-ml-3 gap-1.5" disabled={update.checking || (update.busy && !update.error)} onClick={() => void update.retry()}><RotateCw size={13} aria-hidden="true" />Check again</Button>
          <Button variant="primary" size="compact" className="gap-1.5" aria-label="Update all" disabled={update.busy || update.checking || update.awaitingRetry || !update.rows.some(item => !item.channelError && item.state !== "completed" && update.items.some(offer => offer.id === item.id))} loading={update.busy} onClick={() => void update.updateAll()}>{update.busy ? "Updating components" : "Update all"}</Button>
        </div>
      </div>
  );
}
