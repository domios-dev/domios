import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId, useState, type CSSProperties } from "react";
import { FileUp } from "lucide-react";
import { blueprintSharingApi, parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { AUTOMATION_LABEL, AUTOMATION_PRIMARY, AUTOMATION_TONE, AUTOMATION_TOOL } from "../Automation/styles";
import { DropdownSelect } from "../shared/DropdownMenu";
import { FileChooser } from "../shared/FileChooser";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { BlueprintProjectList, type BlueprintProjectApi } from "./BlueprintProjectList";

export interface BlueprintCreator {
  id: string;
  label: string;
  /** The Team the tab belongs to; the destination list groups by it. */
  group?: string;
}

type ImportSource = "file" | "repository";
const IMPORT_SOURCES: SegmentedOption<ImportSource>[] = [{ value: "file", label: "From file" }, { value: "repository", label: "From repository" }];

export function BlueprintImportForm({ creators, onCreated, onCancel, api = blueprintSharingApi, workspaceIds = [], workspaceName, repositoryApi }: {
  creators: BlueprintCreator[]; onCreated: () => void; onCancel: () => void; api?: BlueprintSharingApi;
  /** The workspaces in view; their repositories offer Blueprints to read. */
  workspaceIds?: readonly string[]; workspaceName?: (id: string) => string; repositoryApi?: BlueprintProjectApi;
}) {
  const prefix = useId();
  const [from, setFrom] = useState<ImportSource>("file");
  const [recipe, setRecipe] = useState<BlueprintRecipe | null>(null);
  const [source, setSource] = useState("");
  const [bindings, setBindings] = useState<Record<string, string>>({});
  const [caller, setCaller] = useState(creators[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const read = async (file: File | undefined) => {
    setRecipe(null);
    setSource("");
    setError(null);
    setBindings({});
    if (!file) return;
    setBusy(true);
    try {
      if (file.size > 256 * 1024) throw new Error("Blueprint recipe exceeds 256 KiB");
      const raw = await file.text();
      const parsed = parseBlueprintRecipe(raw);
      setRecipe(parsed);
      setSource(raw);
      setBindings(Object.fromEntries(Object.entries(parsed.inputs ?? {}).filter(([, input]) => input.default !== undefined).map(([key, input]) => [key, input.type === "string" ? String(input.default) : (JSON.stringify(input.default) ?? "")])));
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  const create = async () => {
    if (!recipe) return;
    setBusy(true);
    setError(null);
    try {
      await api.import(caller, source, Object.keys(recipe.inputs ?? {}).filter((key) => bindings[key] !== undefined).map((key) => `${key}=${bindings[key] ?? ""}`));
      onCreated();
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <form aria-label="Import Blueprint" className="ct-card ct-card--static" style={{ "--ct-card-tone": AUTOMATION_TONE.blueprint } as CSSProperties} onSubmit={(event) => { event.preventDefault(); void create(); }}>
    <div className="ct-card__head">
      <span className="ct-card__icon" aria-hidden="true"><FileUp size={14} /></span>
      <h3 className="ct-detail__heading">Import Blueprint</h3>
    </div>
    <div className="ct-card__body ct-job-section__body">
      <Segmented<ImportSource> aria-label="Import from" value={from} height={28} options={IMPORT_SOURCES} onChange={setFrom} />
      {from === "repository" ? <BlueprintProjectList embedded workspaceIds={workspaceIds} workspaceName={workspaceName} api={repositoryApi} /> : <>
      <div className="flex flex-col gap-1.5">
        <label className={AUTOMATION_LABEL} htmlFor={`${prefix}-file`}>Blueprint YAML file</label>
        <FileChooser id={`${prefix}-file`} accept=".yaml,.yml,.json,application/yaml,application/json,text/yaml" disabled={busy} onFile={(file) => void read(file)} />
      </div>
      {/* The import runs as a tab of the destination Team; the runtime refuses a caller outside a Team, so General is not offered. */}
      <div className="flex flex-col gap-1.5">
        <span className={AUTOMATION_LABEL}>Destination</span>
        <DropdownSelect aria-label="Destination" placeholder={creators.length ? "Choose a Team" : "No Team to import into"} disabled={busy || creators.length === 0}
          value={caller} onChange={setCaller} options={creators.map((creator) => ({ value: creator.id, label: creator.label, group: creator.group }))} />
      </div>
      {recipe && Object.entries(recipe.inputs ?? {}).map(([key, input]) => <div key={key} className="flex flex-col gap-1.5">
        <label className={AUTOMATION_LABEL} htmlFor={`${prefix}-${key}`}>{key}: {input.description ?? input.type}</label>
        <input id={`${prefix}-${key}`} className="ct-field w-full" value={bindings[key] ?? ""} required={input.default === undefined} disabled={busy} onChange={(event) => { const value = event.currentTarget.value; setBindings((current) => ({ ...current, [key]: value })); }} />
      </div>)}
      </>}
      {error && <InlineErrorNotice error={error} operation="importing this Blueprint" blocking />}
      <div className="ct-job-actions">
        {from === "file" && <button type="submit" className={AUTOMATION_PRIMARY} disabled={busy || !recipe || !creators.some((creator) => creator.id === caller)}>Import stopped Blueprint</button>}
        <button type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={onCancel}>Cancel import</button>
      </div>
    </div>
  </form>;
}
