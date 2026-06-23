import { NoteCard } from "./cards/NoteCard";
import { DecisionCard } from "./cards/DecisionCard";
import type { NodeRenderer } from "./nodeRegistry";
import type { CanvasElement } from "./types";

export function registerProvenanceRenderers(register: (kind: string, renderer: NodeRenderer) => void): void {
  register("note", ({ model, style, lod }) =>
    model ? <NoteCard element={model as CanvasElement} style={style} lod={lod} /> : null,
  );
  register("decision", ({ model, style, lod }) =>
    model ? <DecisionCard element={model as CanvasElement} style={style} lod={lod} /> : null,
  );
}
