import { useState } from "react";
import { Ellipsis } from "lucide-react";
import { BusySpinner } from "../shared/BusyState";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";
import { TabMenu } from "./TabMenu";
import type { TabMenuAction, TabMenuEntry } from "./tabMenuModel";

interface TabActionsMenuProps {
  entries: TabMenuEntry[];
  busy: boolean;
  onAction: (action: TabMenuAction) => void;
  onOpen?: () => void;
}

export function TabActionsMenu({ entries, busy, onAction, onOpen }: TabActionsMenuProps) {
  const [open, setOpen] = useState(false);
  return (
    <TabMenu
      entries={entries}
      onAction={onAction}
      menuTestId="tab-actions-menu"
      className="inline-flex"
      onOpenChange={(next) => {
        setOpen(next);
        if (next) onOpen?.();
      }}
      trigger={
        <button
          type="button"
          data-testid="tab-actions-trigger"
          aria-label="Tab actions"
          aria-haspopup="menu"
          aria-expanded={open}
          title="Tab actions"
          className="inline-flex w-7 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
          style={{ height: TAB_BAR_CONTROL_H }}
        >
          <BusySpinner spinning={busy} size={13} icon={Ellipsis} />
        </button>
      }
    />
  );
}
