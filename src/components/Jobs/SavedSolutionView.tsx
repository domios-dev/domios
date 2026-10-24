import { useState } from "react";
import { Download, Trash2 } from "lucide-react";
import { stringify } from "yaml";
import { BlueprintView } from "../Automation/blueprint/BlueprintView";
import { blueprintSharingApi, type BlueprintSharingApi } from "../Automation/blueprintSharing";
import { AUTOMATION_TOOL } from "../Automation/styles";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { blueprintSolutionsApi, type BlueprintSolutionsApi } from "./blueprintSolutions";
import { solutionMeta } from "./SavedSolutions";
import { solutionPipeline, type CatalogEntry } from "./solutionCatalog";

/** A saved solution opened read-only in the single Blueprint view: its graph, ports and code, with Export and Delete. */
export function SavedSolutionView({ entry, backLabel, onBack, onDeleted, api = blueprintSolutionsApi, sharing = blueprintSharingApi }: {
  entry: CatalogEntry; backLabel: string; onBack: () => void; onDeleted: () => void; api?: BlueprintSolutionsApi; sharing?: BlueprintSharingApi;
}) {
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const pipeline = solutionPipeline(entry);
  if (!pipeline || !entry.document) return <InlineErrorNotice error={entry.error ?? "Saved solution unavailable"} operation="opening this saved solution" blocking />;
  const document = entry.document;
  const remove = async () => {
    setDeleting(true); setError(null);
    try { await api.archive(entry.solution.workspaceId, entry.solution.id); onDeleted(); }
    catch (failure) { setError(failure); setDeleting(false); }
  };
  const actions = <>
    <button type="button" className={AUTOMATION_TOOL} onClick={() => sharing.save({ name: document.name, yaml: stringify(document) })}><Download size={13} aria-hidden="true" />Export Blueprint</button>
    <button type="button" className={`${AUTOMATION_TOOL} bp-stop`} disabled={deleting} aria-busy={deleting || undefined} onClick={() => void remove()}><Trash2 size={12} aria-hidden="true" />{deleting ? "Deleting…" : "Delete"}</button>
  </>;
  const about = { purpose: entry.solution.when_to_use, intents: (entry.solution.intents ?? []).filter((intent) => intent !== entry.solution.when_to_use), facts: [solutionMeta(entry)] };
  const notices = error != null ? <InlineErrorNotice error={error} operation="deleting this saved solution" blocking /> : null;
  return <BlueprintView pipeline={pipeline} backLabel={backLabel} onBack={onBack} actions={actions} about={about} hostNotices={notices} history={false} headingLevel={3} fill />;
}
