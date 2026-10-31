import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { BookOpen, Pencil, TriangleAlert } from "lucide-react";
import { Button } from "../shared/Button";
import { CornerResizeGrip, useVerticalResize } from "../shared/CornerResizeGrip";
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
import { PERMISSION_WORKSPACES, type MachineAccess, type NeighborPermission } from "./neighborRuntimeClient";
import { scopeConflicts } from "./neighborScopeConflicts";
import { LiquidToggle } from "../shared/LiquidToggle";
import "./neighborPermissions.css";

const ACCESS_OPTIONS: { value: MachineAccess; label: string }[] = [
  { value: "read_only", label: "Read only" },
  { value: "folder", label: "This folder only" },
  { value: "full", label: "Full access" },
];

const SCOPE_LIMIT = 8192;
const SCOPE_HEIGHT = { min: 72, max: 480 };
const scopeHeightKey = (friendship: string) => `domios.neighbors.scope-height:${friendship}`;

function readScopeHeight(friendship: string): number | null {
  try {
    const value = Number(localStorage.getItem(scopeHeightKey(friendship)));
    return Number.isFinite(value) && value > 0 ? value : null;
  } catch { return null; }
}

function writeScopeHeight(friendship: string, height: number) {
  try { localStorage.setItem(scopeHeightKey(friendship), String(height)); } catch { /* Storage can be unavailable in a private viewer. */ }
}
const samePermission = (a: NeighborPermission, b: NeighborPermission) => a.allowed === b.allowed && a.scope === b.scope
  && a.allow_tab_requests === b.allow_tab_requests && (a.allow_interactive_content ?? false) === (b.allow_interactive_content ?? false)
  && (a.allow_memory ?? true) === (b.allow_memory ?? true) && (a.allow_files ?? true) === (b.allow_files ?? true)
  && (a.folder ?? null) === (b.folder ?? null) && (a.memory_areas ?? []).join("\n") === (b.memory_areas ?? []).join("\n") && (a.machine_access ?? "read_only") === (b.machine_access ?? "read_only")
  && (a.workspaces ?? []).join("\n") === (b.workspaces ?? []).join("\n")
  && (a.memory_write_areas ?? []).join("\n") === (b.memory_write_areas ?? []).join("\n");
const scopeBytes = (scope: string) => new TextEncoder().encode(scope).length;

/** A field that follows its saved value until the owner edits it. Only an edit can stage a write: a saved value that
 *  arrives or changes after mount (the fresh friend list, a change made elsewhere) re-syncs an untouched field. */
function useStagedValue<T>(saved: T) {
  const [value, setValue] = useState(saved);
  const [edited, setEdited] = useState(false);
  useEffect(() => { if (!edited) setValue(saved); }, [saved, edited]);
  return {
    value, edited,
    edit: (next: T) => { setValue(next); setEdited(true); },
    /** Back to the saved value, untouched; after a save, the saved value arrives as the new one. */
    reset: () => { setValue(saved); setEdited(false); },
  };
}

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
  const [savedLimit, setSavedLimit] = useState(neighbor.inbound_per_hour);
  useEffect(() => { setSavedLimit(neighbor.inbound_per_hour); }, [neighbor.inbound_per_hour]);
  const hourly = useStagedValue(savedLimit);
  const limit = hourly.value;
  const { pickFolder, folderPicker } = useFolderPicker();
  const accessLabel = useId();
  const areas = useMemoryAreas(true);
  // The owner's own workspaces; General is the machine's home, not a place to share.
  const allWorkspaces = useMuxSelector(state => state.workspaces);
  const workspaceOptions = allWorkspaces.filter(workspace => !workspace.protected).map(workspace => ({ value: workspace.id, label: workspace.name }));
  const access: MachineAccess = draft?.machine_access ?? "read_only";
  const dailyField = useId();
  const fileField = useId();
  // Compared with what was last saved here too, so clearing a limit just set can still be saved as no limit.
  const [savedDaily, setSavedDaily] = useState<number | null>(neighbor.my_daily_limit ?? null);
  useEffect(() => { setSavedDaily(neighbor.my_daily_limit ?? null); }, [neighbor.my_daily_limit]);
  const dailyInput = useStagedValue(savedDaily === null ? "" : String(savedDaily));
  const daily = dailyInput.value;
  const dailyValue = daily.trim() === "" ? null : Number(daily);
  const dailyValid = dailyValue === null || (Number.isInteger(dailyValue) && dailyValue >= 1 && dailyValue <= 10000);
  const dailyChanged = dailyInput.edited && dailyValue !== savedDaily;
  const [savedFileLimit, setSavedFileLimit] = useState<number | null>(neighbor.my_file_daily_limit_bytes ?? null);
  useEffect(() => { setSavedFileLimit(neighbor.my_file_daily_limit_bytes ?? null); }, [neighbor.my_file_daily_limit_bytes]);
  // An empty field is the default (null), never an explicit 50 MiB, unless the owner types one.
  const fileInput = useStagedValue(mibText(savedFileLimit));
  const fileLimit = fileInput.value;
  const fileMib = fileLimit.trim() === "" ? null : Number(fileLimit);
  const fileValid = fileMib === null || (Number.isFinite(fileMib) && fileMib >= 0 && fileMib <= FILE_LIMIT_MIB);
  const fileBytes = fileMib === null ? null : Math.round(fileMib * MIB);
  const fileChanged = onSaveFileDailyLimit !== undefined && fileInput.edited && fileBytes !== savedFileLimit;
  const limitChanged = hourly.edited && limit !== savedLimit;
  const scopeField = useId();
  // The scope field keeps the height the owner dragged it to, per friend.
  const [storedScopeHeight] = useState(() => readScopeHeight(neighbor.friendship_id));
  const scopeResize = useVerticalResize<HTMLTextAreaElement>({ min: SCOPE_HEIGHT.min, max: SCOPE_HEIGHT.max, initial: storedScopeHeight,
    onResized: height => writeScopeHeight(neighbor.friendship_id, height) });
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
  const conflicts = draft ? scopeConflicts(draft) : [];
  // Every edit waits here until Apply sends it; permission changes restart the concierge, limits do not.
  const staged = changed || dailyChanged || fileChanged || limitChanged;
  const blocked = (changed && problem !== null) || !dailyValid || !fileValid || !validNeighborLimit(limit);
  const apply = () => {
    if (!draft) return;
    setSaving(true); setError(null);
    // The hourly limit reports its own failure; a refusal just leaves it staged.
    const limitSave = async () => { if (await onLimit(limit)) { setSavedLimit(limit); hourly.reset(); } };
    // Only fields the owner edited are sent; each one saved goes back to following its saved value.
    const saves = [changed ? onSave(draft).then(() => setBaseline(draft)) : Promise.resolve(),
      dailyChanged ? onSaveDailyLimit(dailyValue).then(() => { setSavedDaily(dailyValue); dailyInput.reset(); }) : Promise.resolve(),
      fileChanged ? onSaveFileDailyLimit!(fileBytes).then(() => { setSavedFileLimit(fileBytes); fileInput.reset(); }) : Promise.resolve(),
      limitChanged ? limitSave() : Promise.resolve()];
    void Promise.all(saves).catch((failure: unknown) => setError(failure instanceof Error ? failure.message : "Permissions could not be saved."))
      .finally(() => setSaving(false));
  };
  const cancel = () => {
    if (baseline) setDraft(baseline);
    dailyInput.reset();
    fileInput.reset();
    hourly.reset();
    setAwaitingFolder(false); setError(null);
  };
  const edit =(patch: Partial<NeighborPermission>) => setDraft(current => current ? { ...current, ...patch } : current);
  return <div className="neighbor-permissions">
    <Segmented<TrustPreset> aria-label="Trust" className="neighbor-trust" value={awaitingFolder ? "collaborator" : draft ? trustPresetOf(draft, dailyValue) : "custom"} height={28} options={TRUST_OPTIONS}
      onChange={preset => {
        if (!draft || saving || preset === "custom") return;
        const applied = applyTrustPreset(preset, draft);
        edit(applied.patch);
        dailyInput.edit(String(applied.dailyLimit));
        setAwaitingFolder(applied.needsFolder);
      }} />
    <SettingsSection title="Requests">
    <SwitchTile label={`Allow requests from ${name}`} hint="Their agents may ask yours for help."
      checked={draft?.allowed === true} onChange={allowed => { if (draft && !saving) edit({ allowed }); }} />
    <FormRow className="neighbor-field" label="What they may ask" htmlFor={scopeField} hint="Plain words your concierge follows.">
      <div className="neighbor-scope">
        <textarea ref={scopeResize.target} id={scopeField} className={`${UI_INPUT.full} neighbor-textarea`} rows={4} disabled={!draft || saving}
          style={scopeResize.height === null ? undefined : { height: `${scopeResize.height}px` }}
          placeholder={draft ? "Nothing allowed yet" : "Reading their permissions"} value={draft?.scope ?? ""} onChange={event => edit({ scope: event.currentTarget.value })} />
        <CornerResizeGrip vertical aria-label="Resize what they may ask; use arrow keys" {...scopeResize.grip} />
      </div>
    </FormRow>
    <SwitchTile label={`Show interactive content from ${name}`} hint="Runs their html and diagrams. Only for people you trust."
      checked={draft?.allow_interactive_content === true} onChange={allow_interactive_content => { if (draft && !saving) edit({ allow_interactive_content }); }} />
    {PERMISSION_WORKSPACES && draft && <ChoiceMenu label="Workspaces" menuLabel="Workspaces" every="None" noun="workspaces" disabled={saving}
      options={workspaceOptions} chosen={draft.workspaces ?? []} onChange={chosen => edit({ workspaces: chosen.length > 0 ? chosen : null })} />}
    </SettingsSection>
    <SettingsSection title="Memory">
    <ChoiceMenu label="Read" icon={<BookOpen size={14} aria-hidden="true" />} hint="Read information from these memory areas."
      menuLabel="Memory areas" every="All areas" noun="areas" disabled={!draft || saving || draft.allow_memory === false}
      toggle={<LiquidToggle aria-label="Read my memory" checked={draft !== null && draft.allow_memory !== false}
        disabled={!draft || saving} onCheckedChange={allow_memory => edit({ allow_memory })} />}
      options={[...new Set([...(areas ?? []), ...(draft?.memory_areas ?? [])])].sort((a, b) => a.localeCompare(b)).map(area => ({ value: area, label: area }))}
      chosen={draft?.memory_areas ?? []} onChange={chosen => edit({ memory_areas: chosen.length > 0 ? chosen : null })} />
    {draft && <ChoiceMenu label="Write" icon={<Pencil size={14} aria-hidden="true" />} hint="Save or change memory only in these areas. Off until an area is selected."
      menuLabel="Memory write areas" every="Off" noun="areas" disabled={saving}
      options={(areas ?? []).map(area => ({ value: area, label: area }))} chosen={draft.memory_write_areas ?? []}
      onChange={memory_write_areas => edit({ memory_write_areas })} />}
    </SettingsSection>
    <SettingsSection title="Access">
    <SwitchTile label="Read files"
      checked={draft !== null && draft.allow_files !== false} onChange={allow_files => { if (draft && !saving) edit({ allow_files }); }} />
    <div className="neighbor-access">
      <p id={accessLabel} className="neighbor-permissions__label">Machine access</p>
      <Segmented<MachineAccess> aria-label="Machine access" value={access} height={32} options={ACCESS_OPTIONS}
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
      {/* A saved read-only concierge may still write its own pad; the runtime's note for that saved state says so. */}
      {access === "read_only" && !awaitingFolder && (baseline?.machine_access === "read_only" && baseline.folder_enforcement?.writes === "pad_only"
        ? <EnforcementNote enforcement={baseline.folder_enforcement} />
        : <p className="neighbor-note">Looks things up; changes nothing on this machine.</p>)}
      {access === "full" && <p role="note" className="neighbor-note neighbor-access__warning"><TriangleAlert size={14} aria-hidden="true" />Their requests can change anything your agents on this machine can reach.</p>}
      {access === "folder" && <div className="neighbor-folder">
        <div className="min-w-0">
          <p className="neighbor-folder__path" title={draft?.folder ?? undefined}>{draft?.folder ?? "No folder chosen"}</p>
          {draft?.folder_enforcement && <EnforcementNote enforcement={draft.folder_enforcement} />}
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
    <SettingsSection title="Limits">
    <div className="neighbor-limit">
      <FormRow className="neighbor-field" label="Messages per day" htmlFor={dailyField}
        hint={dailyValid ? "Empty for no limit." : "Use a whole number from 1 to 10000, or leave it empty."}>
        <div className="neighbor-limit__row">
          <input id={dailyField} type="number" min={1} max={10000} step={1} inputMode="numeric" placeholder="No limit"
            className={`${UI_INPUT.compact} neighbor-limit__input`} value={daily} aria-invalid={!dailyValid} disabled={saving}
            onChange={event => dailyInput.edit(event.currentTarget.value)} />
        </div>
      </FormRow>
    </div>
    <div className="neighbor-limit">
      <FormRow className="neighbor-field" label="Files per day from this friend" htmlFor={fileField}
        hint={!fileValid ? `Use a number of MiB from 0 to ${FILE_LIMIT_MIB}, or leave it empty.` : savedFileLimit === 0 ? `Files from ${name} are blocked.` : `Empty for ${FILE_LIMIT_MIB} MiB; 0 blocks files.`}>
        <div className="neighbor-limit__row">
          <input id={fileField} type="number" min={0} max={FILE_LIMIT_MIB} step={0.5} inputMode="decimal" placeholder={String(FILE_LIMIT_MIB)}
            className={`${UI_INPUT.compact} neighbor-limit__input`} value={fileLimit} aria-invalid={!fileValid} disabled={!onSaveFileDailyLimit || saving}
            onChange={event => fileInput.edit(event.currentTarget.value)} />
          <span className="neighbor-note">MiB</span>
        </div>
      </FormRow>
    </div>
    <div className="neighbor-limit">
      <FormRow className="neighbor-field" label="Incoming requests per hour" htmlFor={limitField} hint={`They accept up to ${neighbor.outbound_per_hour} per hour from you.`}>
        <div className="neighbor-limit__row">
          <input id={limitField} type="number" min={1} max={200} step={1} className={`${UI_INPUT.compact} neighbor-limit__input`} value={limit} disabled={saving}
            onChange={event => hourly.edit(event.currentTarget.valueAsNumber)} />
        </div>
      </FormRow>
    </div>
    </SettingsSection>
    {problem && changed && <p role="alert" className="neighbor-note">{problem}</p>}
    {/* The concierge follows the switches, so prose that promises more is only pointed out; Apply stays available. */}
    {conflicts.length > 0 && <div role="status" className="neighbor-scope-conflicts">
      {conflicts.map(conflict => <p key={conflict.grant} className="neighbor-note">{conflict.line}</p>)}
    </div>}
    {error && <InlineErrorNotice error={error} operation="saving Neighbor permissions" role="alert" />}
    <div className="neighbor-apply">
      {changed && <p className="neighbor-note neighbor-apply__note">Applying restarts {name}'s concierge with a fresh session.</p>}
      <div className="neighbor-apply__actions">
        <Button size="sm" variant="accent" onClick={onEditWithGeneral}>Edit with General</Button>
        <span className="ml-auto" />
        <Button size="sm" disabled={!staged || saving} onClick={cancel}>Cancel</Button>
        <Button size="sm" variant="primary" loading={saving} disabled={!staged || blocked || pending} onClick={apply}>Apply</Button>
      </div>
    </div>
    {folderPicker}
  </div>;
}

type FolderEnforcement = NonNullable<NeighborPermission["folder_enforcement"]>;

const WRITES_LINE: Record<FolderEnforcement["writes"], string> = {
  pad_only: "Only its own notes folder is writable.",
  provider_sandbox: "Its sandbox keeps changes inside the allowed folder.",
  unrestricted: "It can change files anywhere it can reach.",
  advisory: "This agent has no file sandbox here, so the folder is a guideline, not a limit.",
};

/** How the runtime actually confines the concierge, in one short line; the runtime's full wording, paths included, stays under Details. */
function EnforcementNote({ enforcement }: { enforcement: FolderEnforcement }) {
  const reads = enforcement.reads === "not_confined" && enforcement.writes !== "advisory" && enforcement.writes !== "unrestricted" ? " Reads are not restricted." : "";
  return <div className="neighbor-enforcement">
    <p className="neighbor-note">{WRITES_LINE[enforcement.writes]}{reads}</p>
    {enforcement.note && <details className="neighbor-enforcement__details">
      <summary>Details</summary>
      <p className="neighbor-note select-text [overflow-wrap:anywhere]">{enforcement.note}</p>
    </details>}
  </div>;
}

/** A stored byte limit as the MiB the field shows, empty for the default. */
function mibText(bytes: number | null): string {
  return bytes === null ? "" : String(Math.round((bytes / MIB) * 10) / 10);
}

const MIB = 1024 * 1024;
/** The service default and its ceiling are the same 50 MiB a day. */
const FILE_LIMIT_MIB = 50;

function areaChipLabel(label: string) {
  return label.length > 28 ? `${label.slice(0, 13)}…${label.slice(-12)}` : label;
}

function ChoiceMenu({ label, menuLabel, every, noun, options, chosen, disabled, onChange, icon, hint, toggle }: {
  label: string; menuLabel: string; every: string; noun: string; options: { value: string; label: string }[];
  chosen: string[]; disabled: boolean; onChange: (chosen: string[]) => void;
  icon?: ReactNode; hint?: string; toggle?: ReactNode;
}) {
  const names = chosen.map(value => options.find(option => option.value === value)?.label ?? value);
  const summary = chosen.length === 0 ? every : chosen.length === 1 ? names[0] : `${chosen.length} ${noun}`;
  return <div className="neighbor-areas" role="group" aria-label={`${label} ${noun}`}>
    <div className="min-w-0">
      <span className="neighbor-permissions__label flex items-center gap-2">{icon}{label}</span>
      {hint && <p className="neighbor-note mt-1">{hint}</p>}
    </div>
    <div className="neighbor-areas__controls">
    {toggle}
    <DropdownMenu menuLabel={menuLabel} placement="bottom-end" maxHeight={180} maxWidth={288} disabled={disabled || options.length === 0}
      header={<p className="px-2 py-1 text-[12px] font-normal tracking-normal text-[var(--ct-muted-fg)]">Choose one or more {noun}</p>}
      items={options.map(option => ({ label: option.label, labelContent: <code className="neighbor-area-name">{areaChipLabel(option.label)}</code>, title: option.label,
        checked: chosen.includes(option.value), keepOpen: true,
        onClick: () => onChange(chosen.includes(option.value) ? chosen.filter(entry => entry !== option.value) : [...chosen, option.value]) }))}
      trigger={<button type="button" className="ct-bar-pill" title={names.join(", ") || every} aria-label={`${menuLabel}: ${summary}`}>
        {chosen.length === 1 ? <code className="neighbor-area-name">{areaChipLabel(summary)}</code> : summary}
      </button>} />
    </div>
  </div>;
}
