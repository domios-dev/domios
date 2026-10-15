import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { Tab } from "../../lib/types";

export function RestoredShell({ tab }: { tab: Tab }) {
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rerun = async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try { await invoke("mux_rerun_restored_shell", { tabId: tab.id }); }
    catch (reason) { setError(String(reason)); }
  };
  return <section aria-label="Restored shell" className="flex h-full flex-col items-start justify-center gap-3 p-6 text-[var(--ct-fg)]">
    <p>Shell restored and stopped.</p>
    {tab.close_restore?.command && <pre className="max-w-full whitespace-pre-wrap break-words">{tab.close_restore.command}</pre>}
    <button type="button" disabled={busy} onClick={() => { void rerun(); }}>
      {tab.close_restore?.command ? "Rerun command" : "Start shell"}
    </button>
    {error && <InlineErrorNotice error={error} operation="starting this shell" blocking />}
  </section>;
}
