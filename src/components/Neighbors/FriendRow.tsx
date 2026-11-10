import { useRef } from "react";
import { Pause } from "lucide-react";
import { AccountAvatar } from "../Titlebar/AccountAvatar";
import { accountInitials } from "../Settings/accountModel";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { openNeighbor } from "./neighborFocus";

const ROW = "sidebar-tab sidebar-tab-dimmed group flex items-center hover:bg-[var(--ct-hover)]";

export function FriendRow({ friendshipId, name, photoURL, uiFontScale, touchMode, active, paused, line, tone, items }: {
  friendshipId: string; name: string; photoURL: string | null; uiFontScale?: number; touchMode: boolean;
  active: boolean; paused: boolean; line: string; tone: "muted" | "progress" | "ok"; items: DropdownMenuItem[];
}) {
  const row = useRef<HTMLDivElement>(null);
  const text = touchMode ? pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"])
    : pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rowPadding = touchMode ? "min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-4 py-1.5";
  return <div ref={row} className={`${ROW} ${rowPadding}`} data-active={active || undefined} data-paused={paused || undefined}>
    <button type="button" className={`sidebar-tab-activation flex flex-1 items-center min-w-0 border-0 bg-transparent p-0 text-left text-inherit cursor-pointer ${touchMode ? "gap-2.5" : "gap-2"}`}
      aria-label={`Open ${name}`} aria-current={active ? "page" : undefined}
      onClick={() => openNeighbor(friendshipId)}>
      <span className={`sidebar-tab-body flex-1 min-w-0 ${touchMode ? "gap-x-2.5" : "gap-x-2"}`}>
        <span data-testid="tab-tab-icon" className="sidebar-tab-body__icon sidebar-neighbor-avatar" data-paused={paused || undefined}>
          <AccountAvatar url={photoURL} initial={accountInitials(name)} label="" size="node" />
          {paused && <span className="neighbor-pause-glyph" aria-hidden="true"><Pause size={8} /></span>}</span>
        <span className="sidebar-tab-body__title flex min-w-0 items-center gap-1.5">
          <span className={`${text} block truncate min-w-0 text-[var(--ct-fg)]`}>{name}</span>
        </span>
        <span className="sidebar-tab-body__sub neighbor-status text-[10px] leading-3" title={line}>
          <span className="neighbor-dot" data-tone={tone} aria-hidden="true" />
          <span className={`min-w-0 ${touchMode ? "whitespace-normal [overflow-wrap:anywhere]" : "truncate"}`}>{line}</span>
        </span>
      </span>
    </button>
    <DropdownMenu className={touchMode ? "sr-only" : "sidebar-tab-kebab"} contextMenuTargetRef={row} menuLabel={`${name} actions`}
      items={items}
      trigger={<RowMenuButton label={`Actions for ${name}`} title="Friend actions" touchMode={touchMode} />} />
  </div>;
}
