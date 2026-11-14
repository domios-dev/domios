import { useCallback, useEffect, useId, useState } from "react";
import { Layers, RefreshCw, Trash2 } from "lucide-react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamChannelAction, TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { startErrorView } from "../../lib/teamTemplates/startModel";
import { publishTeamChannels, teamChannelView, useTeamChannels, type TeamChannelView } from "../../lib/teamTemplates/teamChannels";
import { ChannelAddForm, ChannelName } from "../Channels/ChannelParts";
import { channelFormError, type ChannelFormInput } from "../Channels/channelForm";
import { Button } from "../shared/Button";
import { BusySpinner, BusyState } from "../shared/BusyState";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { SettingsCard } from "./shared";

const MUTED = "text-[11px] text-[var(--ct-muted-fg)]";

function updateLine(view: TeamChannelView): string {
  if (!view.autoUpdates) return "Updates only when you refresh";
  // Offline the official channel keeps the teams bundled with Domios, which is a calm fact rather than an error.
  if (view.refresh.kind === "ok") return "Updates on its own · up to date";
  if (view.refresh.kind === "bundled") return "Updates on its own · using the teams bundled with Domios";
  return "Updates on its own";
}

function ChannelRow({ view, busy, onRefresh, onRemove }: {
  view: TeamChannelView;
  busy: string | null;
  onRefresh: () => void;
  onRemove: () => void;
}) {
  const nameId = useId();
  const skippedId = useId();
  const [showSkipped, setShowSkipped] = useState(false);
  const refreshing = busy === `refresh:${view.name}`;
  const skipped = view.skipped.length;
  return (
    <li aria-labelledby={nameId} data-testid="team-channel-row" data-channel-name={view.name}
      className="flex flex-col gap-1.5 border-b border-[var(--ct-border)] px-3 py-2.5 last:border-b-0">
      <div className="flex min-w-0 items-center gap-2.5">
        <div className="min-w-0 flex-1">
          <ChannelName id={nameId} label={view.label} official={view.official} />
          {!view.official && <div className={`${MUTED} mt-0.5 truncate`} title={view.url}>{view.url}</div>}
          <div className={`${MUTED} mt-0.5 truncate`} title={view.refresh.kind === "bundled" ? view.refresh.reason : undefined}>{updateLine(view)}</div>
          {view.refresh.kind === "failed" && (
            <div className="mt-0.5 truncate text-[11px] text-[var(--ct-err)]" title={view.refresh.reason}>Last refresh failed: {view.refresh.reason}</div>
          )}
        </div>
        {skipped > 0 && (
          <button type="button" aria-expanded={showSkipped} aria-controls={skippedId} onClick={() => setShowSkipped((open) => !open)}
            className={`${MUTED} shrink-0 rounded px-1 outline-none hover:text-[var(--ct-fg)] focus-visible:ring-1 focus-visible:ring-[var(--ct-accent)]`}>
            {skipped} skipped
          </button>
        )}
        <Button variant="icon" aria-label={`Refresh ${view.label}`} title="Refresh" disabled={busy !== null} onClick={onRefresh}>
          <BusySpinner icon={RefreshCw} size={13} spinning={refreshing} />
        </Button>
        {view.removable && (
          <Button variant="icon" aria-label={`Remove ${view.label}`} title="Remove" disabled={busy !== null} onClick={onRemove}>
            <Trash2 size={13} aria-hidden="true" />
          </Button>
        )}
      </div>
      {skipped > 0 && showSkipped && (
        <ul id={skippedId} aria-label={`Teams ${view.label} skipped`} className="m-0 flex list-none flex-col gap-1 p-0">
          {view.skipped.map((entry) => (
            <li key={entry.name} className={`${MUTED} break-words`}>
              <span className="font-mono text-[var(--ct-fg)]">{entry.name}</span> — {entry.reason}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function RemoveConfirm({ view, onCancel, onConfirm }: { view: TeamChannelView; onCancel: () => void; onConfirm: () => void }) {
  return (
    <li role="group" aria-label={`Remove ${view.label}`} data-testid="team-channel-remove-confirm"
      className="flex flex-wrap items-center gap-2 border-b border-[var(--ct-border)] px-3 py-2.5 text-[12px] last:border-b-0">
      <span className="min-w-0 flex-1">
        <span className="block font-medium">Remove {view.label}?</span>
        <span className={`block ${MUTED}`}>Its teams leave the gallery. Teams already started keep running.</span>
      </span>
      <Button variant="ghost" className="text-xs" autoFocus onClick={onCancel}>Cancel</Button>
      <Button variant="danger" className="text-xs" onClick={onConfirm}>Remove</Button>
    </li>
  );
}

/** Team channels beside plugin channels: the official domios-teams channel and any community channel the owner adds. */
export function TeamChannelsCard({ transport = teamTemplatesTransport }: { transport?: () => TeamTemplatesTransport }) {
  const rows = useTeamChannels();
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    void Promise.resolve().then(() => transport().channels({ signal: controller.signal })).then(
      (list) => { if (!controller.signal.aborted) publishTeamChannels(list.channels); },
      (error: unknown) => { if (!controller.signal.aborted) setLoadError(error); },
    );
    return () => controller.abort();
  }, [transport]);

  const run = useCallback(async (key: string, action: TeamChannelAction): Promise<string | null> => {
    setBusy(key);
    try {
      const list = await transport().channelAction(action);
      publishTeamChannels(list.channels, { changed: true });
      return null;
    } catch (error) {
      return startErrorView(error).message;
    } finally {
      setBusy(null);
    }
  }, [transport]);

  const add = useCallback(async (input: ChannelFormInput): Promise<boolean> => {
    const invalid = channelFormError(input, { nameRequired: false });
    setAddError(invalid);
    if (invalid) return false;
    const failure = await run("add", { action: "add", url: input.url.trim(), name: input.name.trim() || null });
    setAddError(failure);
    if (failure) return false;
    setAddOpen(false);
    return true;
  }, [run]);

  const act = useCallback(async (key: string, action: TeamChannelAction) => {
    setActionError(await run(key, action));
  }, [run]);

  const views = (rows ?? []).map(teamChannelView);
  return (
    <SettingsCard icon={<Layers size={16} />} title="Team channels"
      description="Where the teams in the gallery come from. Only the official channel updates on its own.">
      <div className="flex flex-col gap-3" data-testid="team-channels">
        {rows === null && !loadError && (
          <DeferredLoader state="present" aria-busy="true"><BusyState inline label="Loading team channels" phase="work" /></DeferredLoader>
        )}
        {rows === null && loadError != null && <InlineErrorNotice error={loadError} operation="loading team channels" />}
        {rows !== null && (
          <ul aria-label="Team channels" className="m-0 flex list-none flex-col overflow-hidden rounded-lg border border-[var(--ct-border)] p-0">
            {views.map((view) => confirming === view.name
              ? <RemoveConfirm key={view.name} view={view} onCancel={() => setConfirming(null)}
                onConfirm={() => { setConfirming(null); void act(`remove:${view.name}`, { action: "remove", name: view.name }); }} />
              : <ChannelRow key={view.name} view={view} busy={busy}
                onRefresh={() => { void act(`refresh:${view.name}`, { action: "refresh", name: view.name }); }}
                onRemove={() => setConfirming(view.name)} />)}
          </ul>
        )}
        {actionError && <p role="alert" className="m-0 text-[12px] text-[var(--ct-err)]">{actionError}</p>}
        <ChannelAddForm testIdPrefix="team-channel" nameRequired={false} focusOnOpen busy={busy !== null} open={addOpen} error={addError}
          onOpenChange={(open) => { setAddOpen(open); setAddError(null); }} onAdd={add} />
      </div>
    </SettingsCard>
  );
}
