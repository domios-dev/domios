import { useEffect, useState } from "react";
import { Archive, BookOpen } from "lucide-react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { JobRow } from "./JobRow";
import { blueprintSolutionsApi, type BlueprintSolutionsApi, type SavedSolution } from "./blueprintSolutions";

export function SavedSolutions({ sources, revision, api = blueprintSolutionsApi }: { sources: readonly string[]; revision: number; api?: BlueprintSolutionsApi }) {
  const [entries, setEntries] = useState<SavedSolution[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [opened, setOpened] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    setEntries([]); setError(null);
    void Promise.all(sources.map(async (workspaceId) => (await api.list(workspaceId)).map((entry) => ({ ...entry, workspaceId })))).then((results) => {
      if (active) setEntries(results.flat());
    }, (error) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [sources, revision, api]);
  const archive = async (entry: SavedSolution) => {
    setBusy(entry.id); setError(null);
    try { await api.archive(entry.workspaceId, entry.id); setEntries((entries) => entries.filter((item) => item.id !== entry.id)); }
    catch (error) { setError(String(error)); }
    finally { setBusy(null); }
  };
  return <section aria-label="Saved solutions" className="mt-6">
    <h3 className="ct-detail__heading mb-3">Saved solutions</h3>
    {error && <InlineErrorNotice error={error} operation="updating saved solutions" blocking />}
    <ul className="flex flex-col gap-2">{entries.map((entry) => <JobRow key={entry.id} jobKey={`solution:${entry.id}`} icon={<BookOpen size={15} aria-hidden="true" />} name={entry.when_to_use} meta={`${entry.uses} uses · ${entry.successes} successes · last used ${entry.last_used ? new Date(entry.last_used * 1000).toLocaleString() : "never"}`} type="Saved solution" tone="blueprint" state="Saved" actionItems={[{ label: "Archive", icon: <Archive size={12} aria-hidden="true" />, disabled: busy !== null, onClick: () => void archive(entry) }]} onOpen={() => setOpened(opened === entry.id ? null : entry.id)} onTriggerRef={() => {}} error={null} />)}</ul>
    {entries.length === 0 && !error && <p className="ct-job-note">No saved solutions.</p>}
    {opened && entries.find((entry) => entry.id === opened)?.how && <p className="ct-job-note mt-3">{entries.find((entry) => entry.id === opened)?.how}</p>}
  </section>;
}
