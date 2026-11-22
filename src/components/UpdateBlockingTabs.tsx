import type { BlockingUpdateTab } from "../lib/updateStatusModel";

export function UpdateBlockingTabs({ tabs }: { tabs: BlockingUpdateTab[] }) {
  if (!tabs.length) return null;
  return <ul aria-label="Tabs blocking the update" className="flex flex-col gap-3 text-xs">
    {tabs.map(tab => <li key={tab.tab_id}>
      <strong>{tab.title}</strong> · {tab.workspace}
      <p>{tab.command}</p>
    </li>)}
  </ul>;
}
