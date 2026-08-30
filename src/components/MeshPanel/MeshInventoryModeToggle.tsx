import { List, Network } from "lucide-react";
import { UI_BUTTON } from "../shared/design";

export type MeshInventoryMode = "list" | "graph";

export function MeshInventoryModeToggle({ mode, onChange }: { mode: MeshInventoryMode; onChange: (mode: MeshInventoryMode) => void }) {
  const nextMode = mode === "graph" ? "list" : "graph";
  return (
    <button
      type="button"
      onClick={() => onChange(nextMode)}
      className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5 backdrop-blur-md shadow-lg`}
      aria-label={nextMode === "list" ? "Show inventory list" : "Show mesh graph"}
      data-testid="mesh-inventory-mode-toggle"
      data-inventory-mode={mode}
    >
      {nextMode === "list" ? <List size={12} aria-hidden="true" /> : <Network size={12} aria-hidden="true" />}
      {nextMode === "list" ? "List" : "Graph"}
    </button>
  );
}
