import { useEffect, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateShareCard } from "../../lib/transport/teamTemplates";
import { startErrorView, templateTitle } from "../../lib/teamTemplates/startModel";
import { shareAsksSummary } from "../../lib/teamTemplates/exchangeModel";
import { openTemplatePreview } from "../../lib/teamTemplates/pendingPreview";
import { TemplateIcon } from "./TemplateIcon";
import { savedFilePath } from "../Neighbors/neighborFilesClient";

type Workspace = { id: string; cwd: string | null; protected?: boolean };
type MuxSlice = { workspaces?: Workspace[]; active_workspace_id?: string | null };

const ACTION = "rounded px-2 py-1 text-[12px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:opacity-50";

/** A Team template a friend sent: checked, never saved or started from here. Preview adds it to My templates and opens its preview. */
export function TemplateShareCardView({ friendship, fileId, senderName, onDismiss, resolvePath = savedFilePath, transport = teamTemplatesTransport }: {
  friendship: string;
  fileId: string;
  senderName: string;
  resolvePath?: (friendship: string, fileId: string) => Promise<string>;
  onDismiss: () => void;
  transport?: () => TeamTemplatesTransport;
}) {
  const [loaded, setLoaded] = useState<{ card: TemplateShareCard; path: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const workspace = useMuxSelector((state: MuxSlice) => {
    const open = (state.workspaces ?? []).filter((candidate) => !candidate.protected);
    return open.find((candidate) => candidate.id === state.active_workspace_id) ?? open[0] ?? null;
  });

  useEffect(() => {
    const controller = new AbortController();
    void resolvePath(friendship, fileId)
      .then((path) => transport().card(fileId, path, { signal: controller.signal }).then((card) => setLoaded({ card, path })))
      .catch((failure: unknown) => { if (!controller.signal.aborted) setError(startErrorView(failure).message); });
    return () => controller.abort();
  }, [friendship, fileId, resolvePath, transport]);

  if (error) return <p className="neighbor-note" role="status" data-testid="template-share-card-error">This team package can't be previewed: {error}</p>;
  if (!loaded || loaded.card.state === "dismissed") return null;
  const { card, path } = loaded;
  const previewed = card.state === "previewed";
  const asks = shareAsksSummary(card.asks ?? []);
  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      const imported = await transport().import(path);
      setLoaded({ card: { ...card, state: "previewed" }, path });
      if (workspace) openTemplatePreview(workspace, { name: imported.template.name, version: imported.template.version }, imported.update ?? null);
    } catch (failure) {
      setError(startErrorView(failure).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <article className="ct-card ct-card--static flex flex-col gap-1.5 p-3" aria-label={`${senderName} shared a team`} data-testid="template-share-card" data-state={card.state}>
      <div className="flex items-start gap-3">
        <TemplateIcon icon={card.template.icon} />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium">{senderName} shared a team</div>
          <div className="truncate text-[12px] text-[var(--ct-muted-fg)]">{templateTitle(card.template)} · {card.roles === 1 ? "1 role" : `${card.roles} roles`}</div>
        </div>
        <span className="flex shrink-0 items-center gap-1.5 text-[12px] text-[var(--ct-muted-fg)]" data-testid="template-share-status">
          {!previewed && <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-yellow,#d97706)]" />}
          {previewed ? "Previewed" : "Unverified"}
        </span>
      </div>
      <div className="pl-11 text-[12px]">{card.template.description}</div>
      {asks && <div className="pl-11 text-[12px] text-[var(--ct-muted-fg)]">{asks}</div>}
      <div className="flex gap-1 pl-9">
        <button type="button" className={ACTION} disabled={busy || !workspace} onClick={() => { void preview(); }}>Preview</button>
        <button type="button" className={ACTION} onClick={onDismiss}>Dismiss</button>
      </div>
      <p className="text-[11px] text-[var(--ct-muted-fg)]">Nothing runs until you preview it and press Start. Previewing adds it to the Mine list.</p>
    </article>
  );
}

const DISMISSED_KEY = "codeterm:team-share-cards-dismissed";
const DISMISSED_LIMIT = 200;

function readDismissed(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
  } catch {
    return [];
  }
}

/** Dismissed share cards stay dismissed across reloads; the file itself stays in the conversation. */
export function useDismissedTemplateCards(): [ReadonlySet<string>, (fileId: string) => void] {
  const [dismissed, setDismissed] = useState<string[]>(readDismissed);
  const dismiss = (fileId: string) => setDismissed((current) => {
    const next = [...current.filter((entry) => entry !== fileId), fileId].slice(-DISMISSED_LIMIT);
    try { localStorage.setItem(DISMISSED_KEY, JSON.stringify(next)); } catch { /* Storage may be unavailable; the card stays dismissed for this view. */ }
    return next;
  });
  return [new Set(dismissed), dismiss];
}
