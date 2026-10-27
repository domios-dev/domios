import { useEffect, useId, useState } from "react";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { SwitchTile } from "../Settings/shared";
import { UI_INPUT } from "../shared/design";
import { validNeighborLimit, type Neighbor } from "./neighborModel";
import type { NeighborPermission } from "./neighborRuntimeClient";

const SCOPE_LIMIT = 8192;
const scopeBytes = (scope: string) => new TextEncoder().encode(scope).length;

/** Allowing requests needs a written scope; the relay refuses an empty or oversized one. */
export function permissionProblem(permission: NeighborPermission): string | null {
  if (scopeBytes(permission.scope) > SCOPE_LIMIT) return "The scope is too long. Keep it to the essentials.";
  if (permission.allowed && !permission.scope.trim()) return "Write what they may ask before allowing requests.";
  return null;
}

/** What a Neighbor may ask, whether they may open tabs, and how often; editable here or with your General Agent. */
export function NeighborPermissionsEditor({ neighbor, name, permission, pending, onSave, onSaveDailyLimit, onLimit, onEditWithGeneral }: {
  neighbor: Neighbor; name: string; permission: NeighborPermission | null | undefined; pending: boolean;
  onSave: (permission: NeighborPermission) => Promise<void>;
  onSaveDailyLimit: (dailyLimit: number | null) => Promise<void>;
  onLimit: (limit: number) => Promise<boolean>; onEditWithGeneral: () => void;
}) {
  const [baseline, setBaseline] = useState<NeighborPermission | null>(permission ?? null);
  const [draft, setDraft] = useState<NeighborPermission | null>(permission ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(neighbor.inbound_per_hour);
  const [daily, setDaily] = useState(neighbor.my_daily_limit == null ? "" : String(neighbor.my_daily_limit));
  const [dailyError, setDailyError] = useState<string | null>(null);
  const dailyField = useId();
  const dailyValue = daily.trim() === "" ? null : Number(daily);
  const dailyValid = dailyValue === null || (Number.isInteger(dailyValue) && dailyValue >= 1 && dailyValue <= 10000);
  const dailyChanged = dailyValue !== (neighbor.my_daily_limit ?? null);
  const scopeField = useId();
  const limitField = useId();
  useEffect(() => { if (permission) { setBaseline(permission); setDraft(permission); } }, [permission]);
  const changed = draft !== null && baseline !== null
    && (draft.allowed !== baseline.allowed || draft.scope !== baseline.scope || draft.allow_tab_requests !== baseline.allow_tab_requests);
  const problem = draft ? permissionProblem(draft) : null;
  const edit = (patch: Partial<NeighborPermission>) => setDraft(current => current ? { ...current, ...patch } : current);
  return <div className="neighbor-permissions">
    <SwitchTile label={`Allow requests from ${name}`} hint="Their agents may ask yours for help within the scope below."
      checked={draft?.allowed === true} onChange={allowed => { if (draft && !saving) edit({ allowed }); }} />
    <FormRow label="What they may ask" htmlFor={scopeField} hint="Plain words your concierge follows, for example: questions about our shared project, never files or secrets.">
      <textarea id={scopeField} className={`${UI_INPUT.full} neighbor-textarea`} rows={4} disabled={!draft || saving}
        placeholder={draft ? "Nothing allowed yet" : "Reading their permissions"} value={draft?.scope ?? ""} onChange={event => edit({ scope: event.currentTarget.value })} />
    </FormRow>
    <SwitchTile label="May ask to open tabs" hint="A separate grant: without it, their agents can only ask questions."
      checked={draft?.allow_tab_requests === true} onChange={allow_tab_requests => { if (draft && !saving) edit({ allow_tab_requests }); }} />
    {problem && changed && <p role="alert" className="neighbor-note">{problem}</p>}
    {error && <InlineErrorNotice error={error} operation="saving Neighbor permissions" role="alert" />}
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" loading={saving} disabled={!changed || problem !== null}
        onClick={() => {
          if (!draft) return;
          setSaving(true); setError(null);
          void onSave(draft).then(() => setBaseline(draft)).catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "Permissions could not be saved."))
            .finally(() => setSaving(false));
        }}>Save permissions</Button>
      <Button size="sm" variant="accent" onClick={onEditWithGeneral}>Edit with General</Button>
    </div>
    <form className="neighbor-limit" onSubmit={event => {
      event.preventDefault();
      if (!dailyValid) return;
      setDailyError(null);
      void onSaveDailyLimit(dailyValue).catch((failure: unknown) => setDailyError(failure instanceof Error ? failure.message : "The daily limit could not be saved."));
    }}>
      <FormRow label="Messages per day" htmlFor={dailyField}
        hint={dailyValid ? `How many messages ${name} may send you a day. Leave empty for no limit.` : "Use a whole number from 1 to 10000, or leave it empty."}>
        <div className="neighbor-limit__row">
          <input id={dailyField} type="number" min={1} max={10000} step={1} inputMode="numeric" placeholder="No limit"
            className={`${UI_INPUT.compact} neighbor-limit__input`} value={daily} aria-invalid={!dailyValid}
            onChange={event => setDaily(event.currentTarget.value)} />
          <Button size="sm" type="submit" disabled={!dailyValid || !dailyChanged}>Save daily limit</Button>
        </div>
      </FormRow>
      {dailyError && <InlineErrorNotice error={dailyError} operation="saving the daily limit" role="alert" />}
    </form>
    <form className="neighbor-limit" onSubmit={event => { event.preventDefault(); if (validNeighborLimit(limit)) void onLimit(limit); }}>
      <FormRow label="Incoming requests per hour" htmlFor={limitField} hint={`They accept up to ${neighbor.outbound_per_hour} per hour from you.`}>
        <div className="neighbor-limit__row">
          <input id={limitField} type="number" min={1} max={200} step={1} className={`${UI_INPUT.compact} neighbor-limit__input`} value={limit}
            onChange={event => setLimit(event.currentTarget.valueAsNumber)} />
          <Button size="sm" type="submit" disabled={pending || !validNeighborLimit(limit)}>Save limit</Button>
        </div>
      </FormRow>
    </form>
  </div>;
}
