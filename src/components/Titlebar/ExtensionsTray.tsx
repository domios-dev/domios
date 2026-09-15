import { ExtensionsIcon } from "../Icons";
import { selectExtension } from "./extensionsTrayLogic";
import { UI_TRAY_BTN } from "../shared/design";

export function ExtensionsTray({ onOpen }: { onOpen: () => void }) {
  return (
    <button
      type="button"
      data-testid="titlebar-extensions"
      onClick={() => selectExtension(null, onOpen)}
      title="Plugins"
      className={UI_TRAY_BTN}
    >
      <ExtensionsIcon size={14} />
    </button>
  );
}
