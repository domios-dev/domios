import { useState } from "react";
import type { ChatCardMenuSpec } from "@codeterm/chat-ui";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";

function ChatCardMenuButton({ label, items }: ChatCardMenuSpec) {
  const [open, setOpen] = useState(false);
  const touch = useTouchOptimizedUi();
  return (
    <DropdownMenu
      menuLabel={label}
      onOpenChange={setOpen}
      className={`ct-card__menu${open || touch ? " is-shown" : ""}`}
      items={items.map((item) => ({
        label: item.label,
        icon: item.icon,
        checked: item.checked,
        onClick: item.onSelect,
        "data-testid": item.testId,
      }))}
      trigger={<RowMenuButton label={label} touchMode={touch} />}
    />
  );
}

export function renderChatCardMenu(menu: ChatCardMenuSpec) {
  return <ChatCardMenuButton label={menu.label} items={menu.items} />;
}
