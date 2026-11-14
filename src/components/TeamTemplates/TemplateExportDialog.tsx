import { useEffect, useId, useState } from "react";
import { FormDialog } from "../shared/FormDialog";
import { Button } from "../shared/Button";
import { LiquidToggle } from "../shared/LiquidToggle";
import { DropdownSelect } from "../shared/DropdownMenu";
import { neighborClient } from "../Neighbors/neighborClient";
import { neighborFilesClient, NeighborFilesFailure } from "../Neighbors/neighborFilesClient";
import type { NeighborReceipt } from "../Neighbors/neighborRuntimeClient";
import type { NeighborList } from "../Neighbors/neighborModel";
import { neighborName } from "../Neighbors/neighborModel";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { ACCOUNT_ENTRY_OPTIONS } from "../Settings/accountSignInOptions";
import { templateShareRefusal, type ShareRefusal } from "./templateShareRefusal";
import { teamTemplatesTransport } from "../../lib/transport/orchestrationRuntime";
import type { TeamTemplatesTransport, TemplateExportResult } from "../../lib/transport/teamTemplates";
import {
  DEFAULT_EXPORT_FORMAT, exportFormats, scrubRows, startErrorView, templateSource, templateTitle,
  type PackageFormat, type ScrubReport, type TemplateSummary,
} from "../../lib/teamTemplates/startModel";

export function ScrubReportCard({ report }: { report: ScrubReport }) {
  const rows = scrubRows(report);
  if (rows.length === 0) return <p className="text-[12px] text-[var(--ct-muted-fg)]" aria-label="Removed before saving">Nothing needed removing.</p>;
  return (
    <ul className="ct-card ct-card--static flex flex-col px-3 py-1" aria-label="Removed before saving">
      {rows.map((row) => (
        <li key={row.label} className="flex min-h-[30px] items-center gap-2.5 border-t border-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] text-[12px] first:border-t-0">
          <span className="w-36 shrink-0">{row.label}</span>
          <span className="min-w-0 flex-1 truncate text-[var(--ct-muted-fg)]" title={row.detail}>{row.detail}</span>
          {row.count !== null && <span className="ct-card__chip font-mono" aria-label={`${row.count} ${row.label.toLowerCase()}`}>{row.count}</span>}
        </li>
      ))}
    </ul>
  );
}

export interface NeighborShare {
  friends: () => Promise<NeighborList>;
  send: (friendship: string, body: string, path: string) => Promise<NeighborReceipt>;
}

const neighborhood: NeighborShare = {
  friends: () => neighborClient.list(),
  send: (friendship, body, path) => neighborFilesClient.send(friendship, body, [path]),
};

export function TemplateExportDialog({ summary, onClose, share: shareInitially = false, transport = teamTemplatesTransport, neighbors = neighborhood }: {
  summary: TemplateSummary;
  onClose: () => void;
  share?: boolean;
  transport?: () => TeamTemplatesTransport;
  neighbors?: NeighborShare;
}) {
  const account = useAccount();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const [shareFailure, setShareFailure] = useState<ShareRefusal | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const titleId = useId();
  const descriptionId = useId();
  const shareable = templateSource(summary).owned;
  const [format, setFormat] = useState<PackageFormat>(DEFAULT_EXPORT_FORMAT);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TemplateExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [share, setShare] = useState(shareInitially && shareable);
  const [friends, setFriends] = useState<NeighborList | null>(null);
  const [friend, setFriend] = useState("");
  const [sent, setSent] = useState<string | null>(null);
  const options = exportFormats(summary).filter((option) => !share || option.format !== "folder");
  const shareFormat = share && format === "folder" ? DEFAULT_EXPORT_FORMAT : format;
  useEffect(() => {
    if (!share || friends) return;
    let live = true;
    neighbors.friends().then((list) => {
      if (!live) return;
      setFriends(list);
    }, () => { if (live) setError("Could not check Neighbors. Close and reopen this dialog to try again."); });
    return () => { live = false; };
  }, [share, friends, neighbors]);
  const recipient = friends?.friends.find((candidate) => candidate.friendship_id === friend);
  const channelNotice: ShareRefusal | null = billing.plan?.unverified === "sign_in_required" ? templateShareRefusal("installation_unverified")
    : billing.plan?.unverified === "unconfirmed" || billing.planLoad === "failed" ? templateShareRefusal("entitlements_unavailable")
      : !billing.plan || billing.plan.verifying ? { message: "Checking sharing access…" }
        : !billing.plan.fullAccess || friends?.access === "locked" ? templateShareRefusal("friends_locked") : null;
  const unavailable = channelNotice?.message
    ?? (recipient?.their_paused ? templateShareRefusal("recipient_paused").message
      : recipient && !recipient.receiving ? templateShareRefusal("recipient_not_receiving").message
        : recipient?.their_file_daily_limit_bytes === 0 ? "This Neighbor does not accept files. Ask them to allow files before sharing." : null);
  const recover = async (action: ShareRefusal["action"]) => {
    if (action === "sign_in") {
      setSigningIn(true);
      try {
        const failure = await account.signIn(ACCOUNT_ENTRY_OPTIONS[0].hint);
        if (failure) { setError("Sign-in could not be completed. Try again."); return; }
        await billing.refresh();
      } finally { setSigningIn(false); }
    } else if (action === "refresh") await billing.refresh();
  };
  const recovery = shareFailure?.action ?? (!result && share ? channelNotice?.action : undefined);
  const submit = async () => {
    if (busy || (share && (!recipient || unavailable))) return;
    setBusy(true);
    setError(null);
    setShareFailure(null);
    try {
      const exported = await transport().export({ template: { name: summary.name, version: summary.version }, format: shareFormat });
      setResult(exported);
      const to = recipient;
      if (share && to && exported.check.valid && exported.path) {
        try {
          const receipt = await neighbors.send(to.friendship_id, `Shared the team ${templateTitle(summary)} ${summary.version}`, exported.path);
          if (receipt.submission === "unconfirmed") {
            setError("Exported, but delivery could not be confirmed. Check the Neighbors conversation before sharing again.");
          } else if (receipt.status === "not_delivered") setShareFailure(templateShareRefusal(receipt.reason));
          else setSent(neighborName(to));
        } catch (failure) {
          setShareFailure(templateShareRefusal(failure instanceof NeighborFilesFailure ? failure.reason : undefined));
        }
      }
    } catch (failure) {
      setError(startErrorView(failure).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-md" stacked onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); if (result) onClose(); else void submit(); }}>
      <h2 id={titleId} className="text-base font-semibold">{share ? "Share" : "Export"} {templateTitle(summary)}</h2>
      <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">Machines, paths and secret values are removed from every export.</p>
      {result ? <>
        <ScrubReportCard report={result.scrub} />
        {!result.check.valid && <ul role="alert" className="text-[12px] text-[var(--ct-red)]">
          {result.check.diagnostics.map((diagnostic) => <li key={`${diagnostic.file}:${diagnostic.path}:${diagnostic.code}`}>{diagnostic.message}</li>)}
        </ul>}
      </> : (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1 text-[11px] text-[var(--ct-muted-fg)]">Format</legend>
          {options.map((option) => (
            <label key={option.format} data-selected={shareFormat === option.format || undefined}
              className={`ct-card ct-card--static ct-card--choice flex items-start gap-2.5 p-2.5 ${option.disabledReason ? "opacity-55" : ""}`}>
              <input type="radio" name="template-format" value={option.format} checked={shareFormat === option.format}
                disabled={Boolean(option.disabledReason)} onChange={() => setFormat(option.format)} className="mt-0.5" />
              <span className="min-w-0 flex-1 text-[12px]">
                <span className="flex flex-wrap items-baseline gap-2"><span className="font-medium">{option.name}</span><span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{option.filename}</span></span>
                <span className="block text-[var(--ct-muted-fg)]">{option.disabledReason ?? option.hint}</span>
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {!result && shareable && (
        <div className="flex flex-col gap-1.5">
          {!shareInitially && <label className="flex items-center justify-between gap-3 text-[12px]">
            Also share to Neighborhood
            <LiquidToggle checked={share} aria-label="Also share to Neighborhood" onCheckedChange={setShare} />
          </label>}
          {share && (friends === null ? error ? null : <p aria-busy="true" className="text-[11px] text-[var(--ct-muted-fg)]">Finding your Neighbors…</p>
            : channelNotice ? null
            : friends.friends.length === 0 ? <p className="text-[11px] text-[var(--ct-muted-fg)]">You have no Neighbors to share with yet.</p>
              : <DropdownSelect aria-label="Share with" placeholder="Choose a recipient" value={friend} onChange={setFriend}
                options={friends.friends.map((candidate) => ({ value: candidate.friendship_id, label: neighborName(candidate) }))} />)}
        </div>
      )}
      {!result && share && unavailable && <p role="status" className="text-[12px] text-[var(--ct-muted-fg)]">{unavailable}</p>}
      {result?.path && <p className="text-[12px] text-[var(--ct-muted-fg)]">Saved to <span className="font-mono">{result.path}</span>{sent && `, and sent to ${sent}`}.</p>}
      {shareFailure && <p role="alert" className="text-[12px] text-[var(--ct-red)]">The template was exported. {shareFailure.message}</p>}
      {recovery && <Button type="button" variant="ghost" className="self-start text-xs" disabled={signingIn || billing.pending || account.model.kind === "pending"}
        onClick={() => void recover(recovery)}>{recovery === "sign_in" ? "Sign in again" : "Retry"}</Button>}
      {error && <p role="alert" className="text-[12px] text-[var(--ct-red)]">{error}</p>}
      <div className="flex justify-end gap-2">
        {!result && <Button type="button" variant="ghost" className="text-xs" onClick={onClose}>Cancel</Button>}
        <Button type="submit" variant="primary" className="text-xs" loading={busy} disabled={!result && share && (!recipient || Boolean(unavailable))}>{result ? "Done" : share ? "Share" : "Export"}</Button>
      </div>
    </FormDialog>
  );
}
