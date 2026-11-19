import { useState } from "react";
import type { BlockingUpdateTab } from "../lib/updateStatusModel";
import { ConfirmDialog } from "./Extensions/ConfirmDialog";
import { Button } from "./shared/Button";

export function UpdateBlockingTabs({ tabs, busy, onStop }: { tabs: BlockingUpdateTab[]; busy: boolean; onStop: (tabId: string) => Promise<void> }) {
  const [pending, setPending] = useState<BlockingUpdateTab | null>(null);
  return <>
    {tabs.map(tab => <div key={tab.tab_id} className="flex items-center justify-between gap-3 text-xs">
      <span><strong>{tab.title}</strong> · {tab.workspace}<br />{tab.command}</span>
      <Button variant="danger" size="compact" disabled={busy} aria-label={`Stop ${tab.title} and retry update`} onClick={() => setPending(tab)}>Stop and retry</Button>
    </div>)}
    {pending && <ConfirmDialog title={`Stop ${pending.title}?`}
      message={`This closes the tab in ${pending.workspace} and stops its work: ${pending.command}. The update will retry after the tab closes.`}
      confirmLabel="Stop and retry" onCancel={() => setPending(null)} onConfirm={() => {
        const tabId = pending.tab_id;
        setPending(null);
        void onStop(tabId);
      }} />}
  </>;
}
