import { useState, type ReactNode } from "react";
import { UpdateTray } from "./UpdateTray";
import { GearIcon } from "../Icons";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { SETTINGS_SECTIONS, type SectionId } from "../Settings/sections";
import { usePwaInstallOffer } from "../../lib/pwaInstallOffer";

interface SettingsMenuProps {
  onOpenSection: (section?: SectionId) => void;
  /** Sits between the gear and the update control, e.g. the account circle. */
  afterGear?: ReactNode;
}

/**
 * Gear button in the titlebar tray that opens the settings menu.
 * Each row deep-links into the full settings panel at that section.
 */
export function SettingsMenu({ onOpenSection, afterGear }: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const installOffer = usePwaInstallOffer();
  const items: DropdownMenuEntry[] = SETTINGS_SECTIONS.map((s) => {
    const Icon = s.icon;
    return {
      label: s.label,
      description: s.desc,
      icon: <Icon size={14} />,
      onClick: () => onOpenSection(s.id),
      "data-testid": `settings-menu-${s.id}`,
    };
  });
  if (installOffer) {
    items.push({ separator: true, key: "pwa-install" });
    items.push(installOffer.kind === "prompt"
      ? { label: "Install Domios", description: installOffer.description, onClick: () => void installOffer.install?.(), "data-testid": "settings-menu-install-domios" }
      : { label: "Add to Home Screen", description: installOffer.description, unavailable: true, "data-testid": "settings-menu-install-domios" });
    items.push({ label: "Do not show again", onClick: installOffer.dismiss, "data-testid": "settings-menu-dismiss-install" });
  }

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
    <UpdateTray />
  </div>;
}
