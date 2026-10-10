import { useId, useState, type CSSProperties } from "react";
import { FileUp } from "lucide-react";
import { blueprintSharingApi, parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { AUTOMATION_LABEL, AUTOMATION_PRIMARY, AUTOMATION_TONE, AUTOMATION_TOOL } from "../Automation/styles";

export interface BlueprintCreator {
  id: string;
  label: string;
}

export function BlueprintImportForm({ creators, onCreated, onCancel, api = blueprintSharingApi }: {
  creators: BlueprintCreator[]; onCreated: () => void; onCancel: () => void; api?: BlueprintSharingApi;
}) {
  const prefix = useId();
  const [recipe, setRecipe] = useState<BlueprintRecipe | null>(null);
  const [bindings, setBindings] = useState<Record<string, string>>({});
  const [caller, setCaller] = useState(creators[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const read = async (file: File | undefined) => {
    setRecipe(null);
    setError(null);
    setBindings({});
    if (!file) return;
    setBusy(true);
    try {
      if (file.size > 256 * 1024) throw new Error("Blueprint recipe exceeds 256 KiB");
      setRecipe(parseBlueprintRecipe(await file.text()));
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  const create = async () => {
    if (!recipe) return;
    setBusy(true);
    setError(null);
    try {
      await api.import(caller, recipe, Object.keys(recipe.placeholders).map((key) => `${key}=${bindings[key] ?? ""}`));
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
      <div className="flex flex-col gap-1.5">
        <label className={AUTOMATION_LABEL} htmlFor={`${prefix}-file`}>Blueprint recipe file</label>
        <input id={`${prefix}-file`} type="file" accept=".blueprint.json,application/json" disabled={busy} onChange={(event) => void read(event.currentTarget.files?.[0])} />
      </div>
      <div className="flex flex-col gap-1.5">
        <label className={AUTOMATION_LABEL} htmlFor={`${prefix}-creator`}>Creator in destination Team</label>
        <select id={`${prefix}-creator`} className="ct-field" value={caller} required disabled={busy} onChange={(event) => setCaller(event.currentTarget.value)}>
          <option value="">Choose a Team tab</option>
          {creators.map((creator) => <option key={creator.id} value={creator.id}>{creator.label}</option>)}
        </select>
      </div>
      {recipe && Object.entries(recipe.placeholders).map(([key, description]) => <div key={key} className="flex flex-col gap-1.5">
        <label className={AUTOMATION_LABEL} htmlFor={`${prefix}-${key}`}>{key}: {description}</label>
        <input id={`${prefix}-${key}`} className="ct-field w-full" value={bindings[key] ?? ""} required disabled={busy} onChange={(event) => { const value = event.currentTarget.value; setBindings((current) => ({ ...current, [key]: value })); }} />
      </div>)}
      {error && <p role="alert" className="ct-job-note">{error}</p>}
      <div className="ct-job-actions">
        <button type="submit" className={AUTOMATION_PRIMARY} disabled={busy || !recipe || !creators.some((creator) => creator.id === caller)}>Create stopped Blueprint</button>
        <button type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={onCancel}>Cancel import</button>
      </div>
    </div>
  </form>;
}
