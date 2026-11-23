import type { Tab } from "../../lib/types";
import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { selectTab } from "../../store/multiplexer/selectors";
import { ProviderUpdateBubble } from "./ProviderUpdateBubble";
import { TabStatusLineModelPicker } from "./StatusLineModelPicker";

export function TabModelUnavailableCard({ tabId, model, detail }: { tabId: string; model?: string; detail?: string }) {
  const tab = useMuxSelector(state => selectTab(state, tabId));
  return <ModelUnavailableCard tab={tab ?? undefined} model={model} detail={detail} />;
}

export function ModelUnavailableCard({ tab, model, detail }: {
  tab?: Tab;
  model?: string;
  detail?: string | null;
}) {
  return <section aria-label="Model unavailable" className="mx-auto my-2 grid w-full max-w-2xl gap-2 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3">
    <p role="status" className="text-sm font-medium">
      {model ? `${model} is unavailable for this account.` : "This model is unavailable for this account."}
    </p>
    {detail && <details className="text-xs text-[var(--ct-muted-fg)]">
      <summary className="cursor-pointer">Provider details</summary>
      <p className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words">{detail}</p>
    </details>}
    {tab && !isReadOnlyTab(tab) && <>
      <div><TabStatusLineModelPicker tab={tab} label={model ?? "Model"} triggerLabel="Choose another model" unavailableModel={model} /></div>
      {tab.agent_type && <ProviderUpdateBubble providerId={tab.agent_type} callerTabId={tab.id} />}
    </>}
  </section>;
}
