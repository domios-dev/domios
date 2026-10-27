import { useEffect, useId, useRef, useState } from "react";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { SettingsSection, SwitchTile } from "../Settings/shared";
import { useFolderPicker } from "../../hooks/useFolderPicker";
import { Segmented } from "../shared/Segmented";
import { DropdownMenu } from "../shared/DropdownMenu";
import { useMemoryAreas } from "./useMemoryAreas";
import { useMuxSelector } from "../../store/multiplexer";
import { applyTrustPreset, TRUST_OPTIONS, trustPresetOf, type TrustPreset } from "./neighborTrustPresets";
import { UI_INPUT } from "../shared/design";
import { validNeighborLimit, type Neighbor } from "./neighborModel";
import type { MachineAccess, NeighborPermission } from "./neighborRuntimeClient";

const ACCESS_OPTIONS: { value: MachineAccess; label: string }[] = [
  { value: "read_only", label: "Read only" },
  { value: "folder", label: "This folder only" },
  { value: "full", label: "Full access" },
];

const SCOPE_LIMIT = 8192;
const samePermission = (a: NeighborPermission, b: NeighborPermission) => a.allowed === b.allowed && a.scope === b.scope
  && a.allow_tab_requests === b.allow_tab_requests && (a.allow_interactive_content ?? false) === (b.allow_interactive_content ?? false)
  && (a.allow_memory ?? true) === (b.allow_memory ?? true) && (a.allow_files ?? true) === (b.allow_files ?? true)
  && (a.folder ?? null) === (b.folder ?? null) && (a.memory_areas ?? []).join("\n") === (b.memory_areas ?? []).join("\n") && (a.machine_access ?? "read_only") === (b.machine_access ?? "read_only")
  && (a.workspaces ?? []).join("\n") === (b.workspaces ?? []).join("\n");
const scopeBytes = (scope: string) => new TextEncoder().encode(scope).length;

/** Allowing requests needs a written scope; the relay refuses an empty or oversized one. */
export function permissionProblem(permission: NeighborPermission): string | null {
  if (scopeBytes(permission.scope) > SCOPE_LIMIT) return "The scope is too long. Keep it to the essentials.";
  if (permission.allowed && !permission.scope.trim()) return "Write what they may ask before allowing requests.";
  if (permission.machine_access === "folder" && !permission.folder) return "Choose the folder their concierge may work in.";
  return null;
}

/** What a Neighbor may ask, whether they may open tabs, and how often; editable here or with your General Agent. */
export function NeighborPermissionsEditor({ neighbor, name, permission, pending, onSave, onSaveDailyLimit, onSaveFileDailyLimit, onLimit, onEditWithGeneral }: {
  neighbor: Neighbor; name: string; permission: NeighborPermission | null | undefined; pending: boolean;
  onSave: (permission: NeighborPermission) => Promise<void>;
  onSaveDailyLimit: (dailyLimit: number | null) => Promise<void>;
  /** Absent until the daemon offers a way to change it; the current allowance still shows. */
  onSaveFileDailyLimit?: (bytes: number | null) => Promise<void>;
  onLimit: (limit: number) => Promise<boolean>; onEditWithGeneral: () => void;
}) {
  const [baseline, setBaseline] = useState<NeighborPermission | null>(permission ?? null);
  const [draft, setDraft] = useState<NeighborPermission | null>(permission ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(neighbor.inbound_per_hour);
  const { pickFolder, folderPicker } = useFolderPicker();
  const accessLabel = useId();
  const areas = useMemoryAreas(draft?.allow_memory !== false);
  // The owner's own workspaces; General is the machine's home, not a place to share.
  const allWorkspaces = useMuxSelector(state => state.workspaces);
  const workspaceOptions = allWorkspaces.filter(workspace => !workspace.protected).map(workspace => ({ value: workspace.id, label: workspace.name }));
  const access: MachineAccess = draft?.machine_access ?? "read_only";
  const [daily, setDaily] = useState(neighbor.my_daily_limit == null ? "" : String(neighbor.my_daily_limit));
  const [dailyError, setDailyError] = useState<string | null>(null);
  const dailyField = useId();
  const dailyValue = daily.trim() === "" ? null : Number(daily);
  const dailyValid = dailyValue === null || (Number.isInteger(dailyValue) && dailyValue >= 1 && dailyValue <= 10000);
  // Compared with what was last saved here too, so clearing a limit just set can still be saved as no limit.
  const [savedDaily, setSavedDaily] = useState<number | null>(neighbor.my_daily_limit ?? null);
  useEffect(() => { setSavedDaily(neighbor.my_daily_limit ?? null); }, [neighbor.my_daily_limit]);
  const dailyChanged = dailyValue !== savedDaily;
  const scopeField = useId();
  const limitField = useId();
  const [awaitingFolder, setAwaitingFolder] = useState(false);
  const [incoming, setIncoming] = useState<NeighborPermission | null>(null);
  const changed = draft !== null && baseline !== null && !samePermission(draft, baseline);
  const dirty = useRef(changed);
  dirty.current = changed;
  // A change saved elsewhere replaces a clean form; with unsaved edits it waits for the owner to choose.
  useEffect(() => {
    if (!permission) return;
    if (dirty.current) { setIncoming(permission); return; }
    setBaseline(permission); setDraft(permission); setIncoming(null);
  }, [permission]);
  const problem = draft ? permissionProblem(draft) : null;
  const edit = (patch: Partial<NeighborPermission>) => setDraft(current => current ? { ...current, ...patch } : current);
  return <div className="neighbor-permissions">
    <Segmented<TrustPreset> aria-label="Trust" value={awaitingFolder ? "collaborator" : draft ? trustPresetOf(draft, dailyValue) : "custom"} height={28} options={TRUST_OPTIONS}
      onChange={preset => {
        if (!draft || saving || preset === "custom") return;
        const applied = applyTrustPreset(preset, draft);
        edit(applied.patch);
        setDaily(String(applied.dailyLimit));
        setAwaitingFolder(applied.needsFolder);
      }} />
    <SettingsSection title="Requests">
    <SwitchTile label={`Allow requests from ${name}`} hint="Their agents may ask yours for help."
      checked={draft?.allowed === true} onChange={allowed => { if (draft && !saving) edit({ allowed }); }} />
    <FormRow label="What they may ask" htmlFor={scopeField} hint="Plain words your concierge follows.">
      <textarea id={scopeField} className={`${UI_INPUT.full} neighbor-textarea`} rows={4} disabled={!draft || saving}
        placeholder={draft ? "Nothing allowed yet" : "Reading their permissions"} value={draft?.scope ?? ""} onChange={event => edit({ scope: event.currentTarget.value })} />
    </FormRow>
    <SwitchTile label={`Show interactive content from ${name}`} hint="Runs their html and diagrams. Only for people you trust."
      checked={draft?.allow_interactive_content === true} onChange={allow_interactive_content => { if (draft && !saving) edit({ allow_interactive_content }); }} />
    {draft && <ChoiceMenu label="Workspaces" menuLabel="Workspaces" every="None" noun="workspaces" disabled={saving}
      options={workspaceOptions} chosen={draft.workspaces ?? []} onChange={chosen => edit({ workspaces: chosen.length > 0 ? chosen : null })} />}
    </SettingsSection>
    <SettingsSection title="Access">
    <SwitchTile label="Read my memory"
      checked={draft !== null && draft.allow_memory !== false} onChange={allow_memory => { if (draft && !saving) edit({ allow_memory }); }} />
    {draft && draft.allow_memory !== false && <MemoryAreas areas={areas} chosen={draft.memory_areas ?? []} disabled={saving}
      onChange={chosen => edit({ memory_areas: chosen.length > 0 ? chosen : null })} />}
    <SwitchTile label="Read files"
      checked={draft !== null && draft.allow_files !== false} onChange={allow_files => { if (draft && !saving) edit({ allow_files }); }} />
    <div className="neighbor-access">
      <p id={accessLabel} className="neighbor-permissions__label">Machine access</p>
      <Segmented<MachineAccess> aria-label="Machine access" value={access} height={26} options={ACCESS_OPTIONS}
        onChange={next => {
          if (!draft || saving) return;
          setAwaitingFolder(false);
          if (next !== "folder" || draft.folder) { edit({ machine_access: next }); return; }
          // This folder only means nothing until a folder is chosen; cancelling keeps the previous access.
          void pickFolder(`Folder for ${name}'s concierge`, draft.effective_folder).then(folder => { if (folder) edit({ machine_access: "folder", folder }); });
        }} />
      {awaitingFolder && <div role="status" className="neighbor-folder neighbor-folder--needed">
        <p className="neighbor-note">Project collaborator works in one folder.</p>
        <Button size="sm" variant="accent" disabled={saving} onClick={() => {
          void pickFolder(`Folder for ${name}'s concierge`, draft?.effective_folder).then(folder => {
            if (!folder) return;
            edit({ machine_access: "folder", folder });
            setAwaitingFolder(false);
          });
        }}>Choose folder</Button>
      </div>}
      {access === "read_only" && !awaitingFolder && <p className="neighbor-note">Looks things up; changes nothing on this machine.</p>}
      {access === "full" && <p role="note" className="neighbor-note neighbor-access__warning">Their requests can change anything your agents on this machine can reach.</p>}
      {access === "folder" && <div className="neighbor-folder">
        <div className="min-w-0">
          <p className="neighbor-folder__path" title={draft?.folder ?? undefined}>{draft?.folder ?? "No folder chosen"}</p>
          {draft?.folder_enforcement?.note && <p className="neighbor-note">{draft.folder_enforcement.note}</p>}
        </div>
        <Button size="sm" disabled={!draft || saving} onClick={() => {
          void pickFolder(`Folder for ${name}'s concierge`, draft?.folder ?? draft?.effective_folder).then(folder => { if (folder) edit({ folder }); });
        }}>Choose folder</Button>
      </div>}
    </div>
    <SwitchTile label="Open tabs and agents"
      checked={draft?.allow_tab_requests === true} onChange={allow_tab_requests => { if (draft && !saving) edit({ allow_tab_requests }); }} />
    </SettingsSection>
    {incoming && !samePermission(incoming, baseline ?? incoming) && <div role="status" className="neighbor-confirm">
      <p>These permissions were changed elsewhere while you were editing.</p>
      <div className="neighbor-confirm__actions">
        <Button size="sm" onClick={() => { setBaseline(incoming); setDraft(incoming); setIncoming(null); }}>Load the new ones</Button>
        <Button size="sm" onClick={() => { setBaseline(incoming); setIncoming(null); }}>Keep my edits</Button>
      </div>
    </div>}
    {problem && changed && <p role="alert" className="neighbor-note">{problem}</p>}
    {error && <InlineErrorNotice error={error} operation="saving Neighbor permissions" role="alert" />}
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" loading={saving} disabled={(!changed && !(dailyChanged && dailyValid)) || problem !== null}
        onClick={() => {
          if (!draft) return;
          setSaving(true); setError(null);
          // A preset also sets the daily message limit, which saves through its own operation.
          const saves = [changed ? onSave(draft).then(() => setBaseline(draft)) : Promise.resolve(),
            dailyChanged && dailyValid ? onSaveDailyLimit(dailyValue).then(() => setSavedDaily(dailyValue)) : Promise.resolve()];
          void Promise.all(saves).catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "Permissions could not be saved."))
            .finally(() => setSaving(false));
        }}>Save permissions</Button>
      <Button size="sm" variant="accent" onClick={onEditWithGeneral}>Edit with General</Button>
    </div>
    <SettingsSection title="Limits">
    <form className="neighbor-limit" onSubmit={event => {
      event.preventDefault();
      if (!dailyValid) return;
      setDailyError(null);
      void onSaveDailyLimit(dailyValue).then(() => setSavedDaily(dailyValue))
        .catch((failure: unknown) => setDailyError(failure instanceof Error ? failure.message : "The daily limit could not be saved."));
    }}>
      <FormRow label="Messages per day" htmlFor={dailyField}
        hint={dailyValid ? "Empty for no limit." : "Use a whole number from 1 to 10000, or leave it empty."}>
        <div className="neighbor-limit__row">
          <input id={dailyField} type="number" min={1} max={10000} step={1} inputMode="numeric" placeholder="No limit"
            className={`${UI_INPUT.compact} neighbor-limit__input`} value={daily} aria-invalid={!dailyValid}
            onChange={event => setDaily(event.currentTarget.value)} />
          <Button size="sm" type="submit" aria-label="Save daily limit" disabled={!dailyValid || !dailyChanged}>Save</Button>
        </div>
      </FormRow>
      {dailyError && <InlineErrorNotice error={dailyError} operation="saving the daily limit" role="alert" />}
    </form>
    <FileDailyLimit neighbor={neighbor} name={name} onSave={onSaveFileDailyLimit} />
    <form className="neighbor-limit" onSubmit={event => { event.preventDefault(); if (validNeighborLimit(limit)) void onLimit(limit); }}>
      <FormRow label="Incoming requests per hour" htmlFor={limitField} hint={`They accept up to ${neighbor.outbound_per_hour} per hour from you.`}>
        <div className="neighbor-limit__row">
          <input id={limitField} type="number" min={1} max={200} step={1} className={`${UI_INPUT.compact} neighbor-limit__input`} value={limit}
            onChange={event => setLimit(event.currentTarget.valueAsNumber)} />
          <Button size="sm" type="submit" aria-label="Save limit" disabled={pending || !validNeighborLimit(limit)}>Save</Button>
        </div>
      </FormRow>
    </form>
    </SettingsSection>
    {folderPicker}
  </div>;
}

const MIB = 1024 * 1024;
/** The service default and its ceiling are the same 50 MiB a day. */
const FILE_LIMIT_MIB = 50;

/** Files a day you accept from a friend, in MiB: empty keeps the 50 MiB default, 0 blocks files. */
function FileDailyLimit({ neighbor, name, onSave }: { neighbor: Neighbor; name: string; onSave?: (bytes: number | null) => Promise<void> }) {
  const current = neighbor.my_file_daily_limit_bytes ?? null;
  const [value, setValue] = useState(current === null ? "" : String(Math.round((current / MIB) * 10) / 10));
  const [error, setError] = useState<string | null>(null);
  const field = useId();
  const mib = value.trim() === "" ? null : Number(value);
  const valid = mib === null || (Number.isFinite(mib) && mib >= 0 && mib <= FILE_LIMIT_MIB);
  const bytes = mib === null ? null : Math.round(mib * MIB);
  const hint = current === 0 ? `Files from ${name} are blocked.` : `Empty for ${FILE_LIMIT_MIB} MiB; 0 blocks files.`;
  return <form className="neighbor-limit" onSubmit={event => {
    event.preventDefault();
    if (!valid || !onSave) return;
    setError(null);
    void onSave(bytes).catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "The file limit could not be saved."));
  }}>
    <FormRow label="Files per day from this friend" htmlFor={field} hint={valid ? hint : `Use a number of MiB from 0 to ${FILE_LIMIT_MIB}, or leave it empty.`}>
      <div className="neighbor-limit__row">
        <input id={field} type="number" min={0} max={FILE_LIMIT_MIB} step={0.5} inputMode="decimal" placeholder={`${FILE_LIMIT_MIB} MiB`}
          className={`${UI_INPUT.compact} neighbor-limit__input`} value={value} aria-invalid={!valid} disabled={!onSave}
          onChange={event => setValue(event.currentTarget.value)} />
        <span className="neighbor-note">MiB</span>
        {onSave && <Button size="sm" type="submit" aria-label="Save file limit" disabled={!valid || bytes === current}>Save</Button>}
      </div>
    </FormRow>
    {error && <InlineErrorNotice error={error} operation="saving the file limit" role="alert" />}
  </form>;
}

/** Narrow what their agents may ask about to a few memory areas; nothing chosen shares every non-secret area. */
function MemoryAreas({ areas, chosen, disabled, onChange }: { areas: string[] | null; chosen: string[]; disabled: boolean; onChange: (chosen: string[]) => void }) {
  const known = [...new Set([...(areas ?? []), ...chosen])].sort((a, b) => a.localeCompare(b));
  return <ChoiceMenu label="Only these areas" menuLabel="Memory areas" every="All areas" noun="areas" disabled={disabled}
    options={known.map(area => ({ value: area, label: area }))} chosen={chosen} onChange={onChange} />;
}

/** A labelled pill that opens the shared menu with one checkbox per choice; nothing chosen means every choice. */
function ChoiceMenu({ label, menuLabel, every, noun, options, chosen, disabled, onChange }: {
  label: string; menuLabel: string; every: string; noun: string; options: { value: string; label: string }[];
  chosen: string[]; disabled: boolean; onChange: (chosen: string[]) => void;
}) {
  const names = chosen.map(value => options.find(option => option.value === value)?.label ?? value);
  const summary = chosen.length === 0 ? every : chosen.length <= 2 ? names.join(", ") : `${chosen.length} ${noun}`;
  return <div className="neighbor-areas">
    <span className="neighbor-permissions__label">{label}</span>
    <DropdownMenu menuLabel={menuLabel} disabled={disabled || options.length === 0}
      items={options.map(option => ({ label: option.label, checked: chosen.includes(option.value), keepOpen: true,
        onClick: () => onChange(chosen.includes(option.value) ? chosen.filter(entry => entry !== option.value) : [...chosen, option.value]) }))}
      trigger={<button type="button" className="ct-bar-pill" aria-label={`${menuLabel}: ${summary}`}>{summary}</button>} />
  </div>;
}
