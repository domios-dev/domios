import { useState, type ReactNode } from "react";
import { UpdateTray } from "./UpdateTray";
import { GearIcon } from "../Icons";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { SETTINGS_SECTIONS, type SectionId } from "../Settings/sections";
import type { FirstRunGate } from "../../lib/firstRunGate";

interface SettingsMenuProps {
  gate?: FirstRunGate;
  onOpenSection: (section?: SectionId) => void;
  /** Sits between the gear and the update control, e.g. the account circle. */
  afterGear?: ReactNode;
}

/**
 * Gear button in the titlebar tray that opens the settings menu.
 * Each row deep-links into the full settings panel at that section.
 */
export function SettingsMenu({ onOpenSection, afterGear, gate }: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const items: DropdownMenuItem[] = SETTINGS_SECTIONS.filter((s) => gate?.allowsSettingsSection(s.id) ?? true).map((s) => {
    const Icon = s.icon;
    return {
      label: s.label,
      description: s.desc,
      icon: <Icon size={14} />,
      onClick: () => onOpenSection(s.id),
      "data-testid": `settings-menu-${s.id}`,
    };
  });

  const gear = (className: string) => (
    <DropdownMenu
      items={items}
      menuLabel="Settings"
      menuTestId="settings-menu"
      onOpenChange={setOpen}
      className="flex"
      trigger={
        <button
          type="button"
          data-testid="titlebar-settings"
          title="Settings"
          aria-haspopup="menu"
          aria-expanded={open}
          className={className}
        >
          <GearIcon size={15} className={open ? "rotate-45 transition-transform" : "transition-transform"} />
        </button>
      }
    />
  );

  return <div className="flex items-center gap-1">
    {gear(`${UI_TRAY_BTN} ${open ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]" : ""}`)}
    {afterGear}
    {(gate?.allows("app") ?? true) && <UpdateTray />}
  </div>;
}
