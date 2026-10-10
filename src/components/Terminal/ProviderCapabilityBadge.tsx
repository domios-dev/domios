import {useCallback, useState} from "react";
import {invoke} from "../../lib/bridge";
import {capabilityBadge} from "../../lib/providerCapabilities";
import {useMuxSelector} from "../../store/multiplexer";
export function ProviderCapabilityBadge({tabId}:{tabId:string}) {
  const snapshot=useMuxSelector(useCallback(state=>state.tabs[tabId]?.provider_signals?.capabilities,[tabId]));
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string | null>(null);
  const label=capabilityBadge(snapshot);
  if (!label) return null;
  return <span role="status" className="inline-flex items-center gap-2 text-xs">
    <span>{error ?? label}</span>
    <button type="button" disabled={busy} aria-label="Refresh provider capabilities" onClick={async()=>{
      setBusy(true); setError(null);
      try { await invoke("refresh_provider_capabilities",{tabId}); }
      catch {setError("Provider capabilities unavailable (refresh failed)");}
      finally {setBusy(false);}
    }}>Refresh</button>
  </span>;
}
