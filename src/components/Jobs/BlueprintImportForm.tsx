import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId, useState, type CSSProperties } from "react";
import { FileUp } from "lucide-react";
import { blueprintSharingApi, parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { AUTOMATION_LABEL, AUTOMATION_PRIMARY, AUTOMATION_TONE, AUTOMATION_TOOL } from "../Automation/styles";
import { DestinationPicker, destinationScope, sameDestination, type Destination, type DestinationKind, type DestinationTeam, type DestinationWorkspace } from "../shared/DestinationPicker";
import { FileChooser } from "../shared/FileChooser";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { BlueprintProjectList, type BlueprintProjectApi } from "./BlueprintProjectList";

export interface BlueprintCreator {
  id: string;
  label: string;
  /** Where the tab lives: its Team, else General or its workspace. An import into that scope runs as this tab. */
  scope?: Destination;
}

type ImportSource = "file" | "repository";
const IMPORT_SOURCES: SegmentedOption<ImportSource>[] = [{ value: "file", label: "From file" }, { value: "repository", label: "From repository" }];

export function BlueprintImportForm({ creators, onCreated, onCancel, api = blueprintSharingApi, workspaceIds = [], workspaceName, repositoryApi, workspaces = [], teams = [], initialDestination = null }: {
  creators: BlueprintCreator[]; onCreated: () => void; onCancel: () => void; api?: BlueprintSharingApi;
  /** The workspaces in view; their repositories offer Blueprints to read. */
  workspaceIds?: readonly string[]; workspaceName?: (id: string) => string; repositoryApi?: BlueprintProjectApi;
  workspaces?: readonly DestinationWorkspace[]; teams?: readonly DestinationTeam[];
  /** Where Jobs was opened; chosen first when a tab there can import. */
  initialDestination?: Destination | null;
}) {
  const prefix = useId();
  const [from, setFrom] = useState<ImportSource>("file");
  const [recipe, setRecipe] = useState<BlueprintRecipe | null>(null);
  const [source, setSource] = useState("");
  const [bindings, setBindings] = useState<Record<string, string>>({});
  // A destination is offered only when a tab already lives there: the runtime imports as that tab.
  const creatorIn = (scope: Destination | null) => scope ? creators.find((creator) => sameDestination(creator.scope ?? null, scope)) : undefined;
  const [caller, setCaller] = useState(() => (creatorIn(initialDestination) ?? creators[0])?.id ?? "");
  const destination = creators.find((creator) => creator.id === caller)?.scope ?? null;
  const offered = { general: creators.some((creator) => creator.scope?.kind === "general"),
    workspaces: workspaces.filter((workspace) => creators.some((creator) => creator.scope?.kind === "workspace" && creator.scope.id === workspace.id)),
    teams: teams.filter((team) => creators.some((creator) => creator.scope?.kind === "team" && creator.scope.id === team.id)) };
  const kinds: DestinationKind[] = [...(offered.general ? ["general" as const] : []), ...(offered.workspaces.length ? ["workspace" as const] : []), ...(offered.teams.length ? ["team" as const] : [])];
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
      await api.import(caller, source, Object.keys(recipe.inputs ?? {}).filter((key) => bindings[key] !== undefined).map((key) => `${key}=${bindings[key] ?? ""}`),
        destination ? destinationScope(destination) : undefined);
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
      <div className="flex flex-col gap-1.5">
        <span className={AUTOMATION_LABEL}>Destination</span>
        <DestinationPicker kinds={kinds} workspaces={[...workspaces.filter((workspace) => workspace.protected), ...offered.workspaces]} teams={offered.teams}
          placeholder={kinds.length ? "Choose a destination" : "No tab to import from"} disabled={busy || kinds.length === 0}
          value={destination} onChange={(next) => setCaller(creatorIn(next)?.id ?? "")} />
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
