import { useState, type ReactNode, type Ref } from "react";
import { UpdateTray } from "./UpdateTray";
import { GearIcon } from "../Icons";
import { Ellipsis } from "lucide-react";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { SETTINGS_SECTIONS, type SectionId } from "../Settings/sections";
import { usePwaInstallOffer } from "../../lib/pwaInstallOffer";

interface SettingsMenuProps {
  onOpenSection?: (section?: SectionId) => void;
  overflowActions?: DropdownMenuEntry[];
  triggerRef?: Ref<HTMLButtonElement>;
  /** Sits between the gear and the update control, e.g. the account circle. */
  afterGear?: ReactNode;
}

/**
 * Gear button in the titlebar tray that opens the settings menu.
 * Each row deep-links into the full settings panel at that section.
 */
export function SettingsMenu({ onOpenSection, afterGear, overflowActions, triggerRef }: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const installOffer = usePwaInstallOffer();
  const settings: DropdownMenuEntry[] = onOpenSection ? SETTINGS_SECTIONS.map((s) => {
    const Icon = s.icon;
    return {
      label: s.label,
      description: s.desc,
      icon: <Icon size={14} />,
      onClick: () => onOpenSection(s.id),
      "data-testid": `settings-menu-${s.id}`,
    };
  }) : [];
  if (installOffer) {
    if (settings.length) settings.push({ separator: true, key: "pwa-install" });
    settings.push(installOffer.kind === "prompt"
      ? { label: "Install Domios", description: installOffer.description, onClick: () => void installOffer.install?.(), "data-testid": "settings-menu-install-domios" }
      : { label: "Add to Home Screen", description: installOffer.description, unavailable: true, "data-testid": "settings-menu-install-domios" });
    settings.push({ label: "Do not show again", onClick: installOffer.dismiss, "data-testid": "settings-menu-dismiss-install" });
  }
  const mobile = overflowActions !== undefined;
  const menuLabel = mobile ? "More actions" : "Settings";
  const items: DropdownMenuEntry[] = overflowActions && settings.length
    ? [...overflowActions, { label: "Settings", icon: <GearIcon size={14} />, submenu: { label: "Settings", items: settings } }]
    : overflowActions ?? settings;

  const gear = (className: string) => (
    <DropdownMenu
      items={items}
      menuLabel={menuLabel}
      menuTestId="settings-menu"
      onOpenChange={setOpen}
      className="flex"
      trigger={
        <button
          ref={triggerRef}
          type="button"
          data-testid="titlebar-settings"
          title={menuLabel}
          aria-label={menuLabel}
          aria-haspopup="menu"
          aria-expanded={open}
          className={className}
        >
          {mobile ? <Ellipsis size={16} aria-hidden /> : <GearIcon size={15} className={open ? "rotate-45 transition-transform" : "transition-transform"} />}
        </button>
      }
    />
  );

  return <div className="flex items-center gap-1">
    {gear(`${UI_TRAY_BTN} ${open ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]" : ""}`)}
    {afterGear}
    {onOpenSection && <UpdateTray />}
  </div>;
}
