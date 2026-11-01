import { useId, useState } from "react";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { FlagCategory, MessageFlag } from "./neighborConciergeClient";

export const FLAG_LABEL: Record<FlagCategory, string> = {
  suspicious_instructions: "Suspicious instructions", private_data_request: "Asks for private data",
  malicious_link: "Malicious link", spam: "Spam", other: "Other",
};

const FLAGGED_BY: Record<MessageFlag["by"], string> = { concierge: "your concierge", tab: "your agent", owner: "you" };

/** "Flagged by your concierge: Spam" for the newest flag; every flag's reason is in the tooltip. */
export function flagLine(flags: readonly MessageFlag[]): { text: string; detail: string } {
  const newest = flags.reduce((latest, flag) => flag.at > latest.at ? flag : latest);
  return { text: `Flagged by ${FLAGGED_BY[newest.by]}: ${FLAG_LABEL[newest.category]}`,
    detail: flags.map(flag => `${FLAG_LABEL[flag.category]}, by ${FLAGGED_BY[flag.by]}: ${flag.reason}`).join("\n") };
}

const REASON_LIMIT = 280;

/** You flag a message from them with a category and a short reason. */
export function NeighborFlagDialog({ name, onFlag, onClose }: {
  name: string; onFlag: (category: FlagCategory, reason: string) => Promise<void>; onClose: () => void;
}) {
  const title = useId();
  const description = useId();
  const reasonId = useId();
  const [category, setCategory] = useState<FlagCategory>("suspicious_instructions");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const valid = reason.trim().length > 0;
  return <FormDialog titleId={title} descriptionId={description} className="max-w-md" onCancel={() => { if (!busy) onClose(); }} onSubmit={event => {
    event.preventDefault();
    if (!valid || busy) return;
    setBusy(true); setError(null);
    void onFlag(category, reason.trim()).then(onClose,
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "The message could not be flagged.")).finally(() => setBusy(false));
  }}>
    <h2 id={title} className="font-semibold">Flag this message</h2>
    <p id={description} className="text-sm text-[var(--ct-muted-fg)]">Your concierge treats a flagged message from {name} with care and does not act on it.</p>
    <DropdownSelect aria-label="Category" value={category} disabled={busy}
      options={(Object.keys(FLAG_LABEL) as FlagCategory[]).map(value => ({ value, label: FLAG_LABEL[value] }))}
      onChange={value => setCategory(value as FlagCategory)} />
    <label className="flex flex-col gap-1 text-xs text-[var(--ct-muted-fg)]" htmlFor={reasonId}>
      Reason
      <textarea id={reasonId} className="ct-field min-h-16 w-full text-sm" maxLength={REASON_LIMIT} value={reason} disabled={busy}
        onChange={event => setReason(event.currentTarget.value)} />
      <span>{reason.length}/{REASON_LIMIT}</span>
    </label>
    {error && <InlineErrorNotice error={error} operation="flagging this message" role="alert" />}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={busy} onClick={onClose}>Cancel</Button>
      <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!valid}>Flag message</Button>
    </div>
  </FormDialog>;
}
