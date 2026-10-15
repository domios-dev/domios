import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import { blueprintSharingApi, type BlueprintSharingApi } from "./blueprintSharing";
import { AUTOMATION_TOOL } from "./styles";

export function BlueprintExportButton({ workspaceId, teamId, id, api = blueprintSharingApi }: {
  workspaceId: string; teamId: string | null; id: string; api?: BlueprintSharingApi;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    setError(null);
    try { api.save(await api.export(workspaceId, teamId, id)); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <>
    <button type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={() => void run()}>Export Blueprint</button>
    {error && <InlineErrorNotice error={error} operation="exporting this Blueprint" blocking />}
  </>;
}
