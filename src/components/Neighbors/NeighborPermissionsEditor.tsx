import { useEffect, useId, useState } from "react";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { LiquidFlip } from "../shared/LiquidToggle";
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
export function NeighborPermissionsEditor({ neighbor, name, permission, pending, onSave, onLimit, onEditWithGeneral }: {
  neighbor: Neighbor; name: string; permission: NeighborPermission | null | undefined; pending: boolean;
  onSave: (permission: NeighborPermission) => Promise<void>;
  onLimit: (limit: number) => Promise<boolean>; onEditWithGeneral: () => void;
}) {
  const [baseline, setBaseline] = useState<NeighborPermission | null>(permission ?? null);
  const [draft, setDraft] = useState<NeighborPermission | null>(permission ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(neighbor.inbound_per_hour);
  const scopeField = useId();
  const limitField = useId();
  const allowLabel = useId();
  const tabsLabel = useId();
  useEffect(() => { if (permission) { setBaseline(permission); setDraft(permission); } }, [permission]);
  const changed = draft !== null && baseline !== null
    && (draft.allowed !== baseline.allowed || draft.scope !== baseline.scope || draft.allow_tab_requests !== baseline.allow_tab_requests);
  const problem = draft ? permissionProblem(draft) : null;
  const edit = (patch: Partial<NeighborPermission>) => setDraft(current => current ? { ...current, ...patch } : current);
  return <div className="neighbor-permissions">
    <div className="neighbor-permissions__row">
      <span id={allowLabel} className="neighbor-permissions__label">Allow requests from {name}</span>
      <LiquidFlip aria-labelledby={allowLabel} className="shrink-0 text-[10px]" checked={draft?.allowed === true} disabled={!draft || saving}
        onCheckedChange={allowed => edit({ allowed })} />
    </div>
    <FormRow label="What they may ask" htmlFor={scopeField} hint="Plain words your concierge follows, for example: questions about our shared project, never files or secrets.">
      <textarea id={scopeField} className={`${UI_INPUT.full} neighbor-textarea`} rows={4} disabled={!draft || saving}
        placeholder={draft ? "Nothing allowed yet" : "Reading their permissions"} value={draft?.scope ?? ""} onChange={event => edit({ scope: event.currentTarget.value })} />
    </FormRow>
    <div className="neighbor-permissions__row">
      <span id={tabsLabel} className="neighbor-permissions__label">They may ask to open tabs</span>
      <LiquidFlip aria-labelledby={tabsLabel} className="shrink-0 text-[10px]" checked={draft?.allow_tab_requests === true} disabled={!draft || saving}
        onCheckedChange={allow_tab_requests => edit({ allow_tab_requests })} />
    </div>
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
