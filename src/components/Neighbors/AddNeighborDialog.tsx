import { useId, useState } from "react";
import { Check, Gift, Plus, UserRound } from "lucide-react";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { FormRow } from "../shared/FormRow";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_INPUT } from "../shared/design";

/** Invite one person by their Domios email; the same confirmation shows whether or not they have an account. */
export function AddNeighborDialog({ pending, error, onSend, onClose }: {
  pending: boolean;
  error: string | null;
  onSend: (email: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const title = useId();
  const description = useId();
  const field = useId();
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState<string | null>(null);
  const address = email.trim();
  return <FormDialog titleId={title} descriptionId={description} className="max-w-md neighbor-dialog" onCancel={onClose} onSubmit={event => {
    event.preventDefault();
    if (sent || pending || !address) return;
    void onSend(address).then(ok => { if (ok) setSent(address); });
  }}>
    <div className="neighbor-dialog__art" aria-hidden="true">
      <span className="neighbor-dialog__you"><UserRound size={18} /></span>
      <span className="neighbor-dialog__link" />
      <span className={`neighbor-dialog__them${sent ? " neighbor-dialog__them--sent" : ""}`}>{sent ? <Check size={18} /> : <Plus size={18} />}</span>
    </div>
    <h2 id={title} className="neighbor-dialog__title">Add a Neighbor</h2>
    <p id={description} className="neighbor-dialog__lead">Your agents can message theirs across machines, end-to-end encrypted. You decide what they may ask.</p>
    {sent ? <p role="status" className="neighbor-dialog__sent">Request saved for <strong>{sent}</strong>. Your General Agent will ask what they may do.</p>
      : <FormRow label="Their Domios email" htmlFor={field} hint="For privacy, you see the same confirmation whether or not they already use Domios.">
        <input id={field} type="email" required maxLength={254} autoComplete="email" placeholder="friend@example.com" className={UI_INPUT.full}
          value={email} onChange={event => setEmail(event.currentTarget.value)} />
      </FormRow>}
    <div className="ct-card ct-card--static neighbor-perk">
      <span className="ct-card__icon"><Gift size={14} aria-hidden="true" /></span>
      <div className="min-w-0">
        <p className="ct-card__title">A free week for them, a free month for you</p>
        <p className="ct-card__meta">Eligible new friends get 7 days of Domios with no payment method. If they subscribe in that week, you earn one free month.</p>
      </div>
    </div>
    {error && <InlineErrorNotice error={error} operation="adding a Neighbor" role="alert" />}
    <div className="flex justify-end gap-2">
      {sent ? <>
        <Button size="sm" onClick={() => { setSent(null); setEmail(""); }}>Add another</Button>
        <Button size="sm" variant="primary" onClick={onClose}>Done</Button>
      </> : <>
        <Button size="sm" onClick={onClose}>Cancel</Button>
        <Button size="sm" variant="primary" type="submit" loading={pending} disabled={!address}>Send request</Button>
      </>}
    </div>
  </FormDialog>;
}
