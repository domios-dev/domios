import { BLUEPRINT_TAGS } from "../../lib/blueprintTags";
import { useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { AUTOMATION_PRIMARY, AUTOMATION_TOOL } from "../Automation/styles";
import { blueprintSolutionsApi, type BlueprintSolutionsApi, type SolutionScope } from "./blueprintSolutions";

export function SaveSolutionForm({ scope, onSaved, onCancel, api = blueprintSolutionsApi }: { scope: SolutionScope; onSaved: () => void; onCancel: () => void; api?: BlueprintSolutionsApi }) {
  const [tags, setTags] = useState<string[]>([]);
  const [when, setWhen] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (!when.trim() || tags.length < 1 || tags.length > 3) return;
    setBusy(true); setError(null);
    try {
      const result = await api.save(scope, when.trim(), tags);
      if (result.status !== "saved" || !result.id) throw new Error("The solution was not persisted.");
      onSaved();
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <form aria-label="Save as solution" className="ct-card ct-card--static my-3 p-3" onSubmit={(event) => { event.preventDefault(); void save(); }}>
    <label className="flex flex-col gap-2">When to use<input autoFocus required maxLength={200} value={when} disabled={busy} onChange={(event) => setWhen(event.target.value)} placeholder="What problem does this solve?" /></label>
    <fieldset disabled={busy} className="mt-3"><legend>Tags (choose 1-3)</legend><div className="flex max-h-48 flex-wrap gap-2 overflow-auto">{BLUEPRINT_TAGS.map((tag) => <label key={tag.name} title={tag.description} className="flex items-center gap-1"><input type="checkbox" checked={tags.includes(tag.name)} disabled={!tags.includes(tag.name) && tags.length === 3} onChange={(event) => setTags(event.target.checked ? [...tags, tag.name] : tags.filter((name) => name !== tag.name))} />{tag.name}</label>)}</div></fieldset>
    <div className="mt-3 flex gap-2"><button type="submit" className={AUTOMATION_PRIMARY} disabled={busy || !when.trim() || tags.length === 0}>{busy ? "Saving…" : "Save as solution"}</button><button type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={onCancel}>Cancel</button></div>
    {error && <InlineErrorNotice error={error} operation="saving this solution" blocking />}
  </form>;
}
