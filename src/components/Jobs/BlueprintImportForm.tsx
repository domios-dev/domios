import { useId, useState } from "react";
import { blueprintSharingApi, parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { AUTOMATION_TOOL } from "../Automation/styles";

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
  return <form aria-label="Import Blueprint" className="space-y-3 rounded-lg border border-[var(--ct-border)] p-4" onSubmit={(event) => { event.preventDefault(); void create(); }}>
    <h3 className="font-semibold">Import Blueprint</h3>
    <label className="block" htmlFor={`${prefix}-file`}>Blueprint recipe file</label>
    <input id={`${prefix}-file`} type="file" accept=".blueprint.json,application/json" disabled={busy} onChange={(event) => void read(event.currentTarget.files?.[0])} />
    <label className="block" htmlFor={`${prefix}-creator`}>Creator in destination Team</label>
    <select id={`${prefix}-creator`} value={caller} required disabled={busy} onChange={(event) => setCaller(event.currentTarget.value)}>
      <option value="">Choose a Team tab</option>
      {creators.map((creator) => <option key={creator.id} value={creator.id}>{creator.label}</option>)}
    </select>
    {recipe && Object.entries(recipe.placeholders).map(([key, description]) => <div key={key}>
      <label className="block" htmlFor={`${prefix}-${key}`}>{key}: {description}</label>
      <input id={`${prefix}-${key}`} className="w-full rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1" value={bindings[key] ?? ""} required disabled={busy} onChange={(event) => { const value = event.currentTarget.value; setBindings((current) => ({ ...current, [key]: value })); }} />
    </div>)}
    {error && <p role="alert">{error}</p>}
    <p className="text-xs text-[var(--ct-muted-fg)]">The imported Blueprint starts stopped.</p>
    <div className="flex gap-2">
      <button type="submit" className={AUTOMATION_TOOL} disabled={busy || !recipe || !creators.some((creator) => creator.id === caller)}>Create stopped Blueprint</button>
      <button type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={onCancel}>Cancel import</button>
    </div>
  </form>;
}
