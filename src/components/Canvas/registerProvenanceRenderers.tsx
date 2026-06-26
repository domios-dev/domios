import { NoteCard } from "./cards/NoteCard";
import { DecisionCard } from "./cards/DecisionCard";
import { HandoffCoin } from "./cards/HandoffCoin";
import type { NodeRenderer } from "./nodeRegistry";
import type { CanvasElement } from "./types";

export function registerProvenanceRenderers(register: (kind: string, renderer: NodeRenderer) => void): void {
  register("note", ({ model, style, lod, world }) =>
    model ? <NoteCard element={model as CanvasElement} style={style} lod={lod} world={world} /> : null,
  );
  register("decision", ({ model, style, lod, world }) =>
    model ? <DecisionCard element={model as CanvasElement} style={style} lod={lod} world={world} /> : null,
  );
  register("handoff", ({ model, style, world }) =>
    model ? <HandoffCoin element={model as CanvasElement} style={style} world={world} /> : null,
  );
}
