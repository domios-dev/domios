import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId, useState, type ReactNode } from "react";
import { blueprintSharingApi, parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { DestinationPicker, destinationScope, sameDestination, type Destination, type DestinationKind, type DestinationTeam, type DestinationWorkspace } from "../shared/DestinationPicker";
import { Button } from "../shared/Button";
import { FileChooser } from "../shared/FileChooser";
import { FormDialog } from "../shared/FormDialog";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { UI_INPUT } from "../shared/design";
import { BlueprintProjectList, type BlueprintProjectApi } from "./BlueprintProjectList";

export interface BlueprintCreator {
  id: string;
  label: string;
  /** Where the tab lives: its Team, else General or its workspace. An import into that scope runs as this tab. */
  scope?: Destination;
}

type ImportSource = "file" | "repository";
const IMPORT_SOURCES: SegmentedOption<ImportSource>[] = [{ value: "file", label: "From file" }, { value: "repository", label: "From repository" }];

/** One plain section of the dialog: a quiet label, the control, and a line of help under it. */
function Section({ label, labelFor, help, children }: { label: string; labelFor?: string; help?: ReactNode; children: ReactNode }) {
  return <section className="flex min-w-0 flex-col gap-1.5">
    {labelFor ? <label className="text-xs font-medium text-[var(--ct-muted-fg)]" htmlFor={labelFor}>{label}</label>
      : <h3 className="text-xs font-medium text-[var(--ct-muted-fg)]">{label}</h3>}
    {children}
    {help && <p className="text-[11px] leading-snug text-[var(--ct-muted-fg)]">{help}</p>}
  </section>;
}

export function BlueprintImportForm({ creators, onCreated, onCancel, api = blueprintSharingApi, workspaceIds = [], workspaceName, repositoryApi, workspaces = [], teams = [], initialDestination = null }: {
  creators: BlueprintCreator[]; onCreated: () => void; onCancel: () => void; api?: BlueprintSharingApi;
  /** The workspaces in view; their repositories offer Blueprints to read. */
  workspaceIds?: readonly string[]; workspaceName?: (id: string) => string; repositoryApi?: BlueprintProjectApi;
  workspaces?: readonly DestinationWorkspace[]; teams?: readonly DestinationTeam[];
  /** Where Jobs was opened; chosen first when a tab there can import. */
  initialDestination?: Destination | null;
}) {
  const prefix = useId();
  const title = useId();
  const description = useId();
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
  const inputs = Object.entries(recipe?.inputs ?? {});
  const valid = from === "file" && recipe !== null && creators.some((creator) => creator.id === caller)
    && inputs.every(([key, input]) => input.default !== undefined || (bindings[key] ?? "").trim() !== "");
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
    if (!recipe || !valid) return;
    setBusy(true);
    setError(null);
    try {
      await api.import(caller, source, inputs.map(([key]) => key).filter((key) => bindings[key] !== undefined).map((key) => `${key}=${bindings[key] ?? ""}`),
        destination ? destinationScope(destination) : undefined);
      onCreated();
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <FormDialog titleId={title} descriptionId={description} className="max-w-lg" onCancel={() => { if (!busy) onCancel(); }}
    onSubmit={(event) => { event.preventDefault(); void create(); }}>
    <header className="flex flex-col gap-1">
      <h2 id={title} className="font-semibold">Import Blueprint</h2>
      <p id={description} className="text-sm text-[var(--ct-muted-fg)]">It arrives stopped, so you can review it before it runs.</p>
    </header>
    <Segmented<ImportSource> aria-label="Import from" className="self-start" value={from} options={IMPORT_SOURCES} onChange={setFrom} />
    {from === "repository"
      ? <Section label="Source" help="Blueprints found in this scope's repositories. Open one to read it.">
        <BlueprintProjectList embedded workspaceIds={workspaceIds} workspaceName={workspaceName} api={repositoryApi} />
      </Section>
      : <>
        <Section label="Source" labelFor={`${prefix}-file`} help="A YAML or JSON recipe, up to 256 KiB. Domios checks it when you import.">
          <FileChooser id={`${prefix}-file`} prompt="Drop a Blueprint YAML or choose a file…" accept=".yaml,.yml,.json,application/yaml,application/json,text/yaml"
            disabled={busy} onFile={(file) => void read(file)} />
        </Section>
        <Section label="Destination" help={kinds.length ? "The Blueprint is created there, as a tab that already lives there." : "Open a tab in General, a workspace or a Team to import into it."}>
          <DestinationPicker kinds={kinds} workspaces={[...workspaces.filter((workspace) => workspace.protected), ...offered.workspaces]} teams={offered.teams}
            placeholder={kinds.length ? "Choose a destination" : "No tab to import from"} disabled={busy || kinds.length === 0}
            value={destination} onChange={(next) => setCaller(creatorIn(next)?.id ?? "")} />
        </Section>
        {inputs.map(([key, input]) => <Section key={key} label={key} labelFor={`${prefix}-${key}`} help={input.description}>
          <input id={`${prefix}-${key}`} className={UI_INPUT.full} value={bindings[key] ?? ""} required={input.default === undefined} disabled={busy}
            onChange={(event) => { const value = event.currentTarget.value; setBindings((current) => ({ ...current, [key]: value })); }} />
        </Section>)}
      </>}
    {error && <InlineErrorNotice error={error} operation="importing this Blueprint" role="alert" />}
    <footer className="flex justify-end gap-2 pt-1">
      <Button size="sm" disabled={busy} onClick={onCancel}>Cancel</Button>
      {from === "file" && <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!valid}>Import</Button>}
    </footer>
  </FormDialog>;
}
