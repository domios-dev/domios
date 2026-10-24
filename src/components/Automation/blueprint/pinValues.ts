import type { BlueprintGraphModel, BlueprintGraphNode, BlueprintGraphWire, BlueprintPin } from "./graphModel";

/** One port's value as a run recorded it: a bounded preview, never the full payload. */
export interface PortValue {
  preview: string;
  type?: string;
  truncated?: boolean;
  bytes?: number;
  redacted?: boolean;
}

export type PinValue =
  | { kind: "value"; value: PortValue }
  | { kind: "summary"; text: string }
  | { kind: "none" };

export const PREVIEW_LIMIT = 1600;

/** The port a data pin stands for: bound inputs by their bind name, typed ports by theirs. */
export function portName(pin: BlueprintPin): string {
  if (pin.id.startsWith("bind:")) return pin.id.slice("bind:".length);
  if (pin.id.startsWith("data:")) return pin.id.slice("data:".length);
  return pin.id;
}

function portValue(raw: unknown): PortValue | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.preview !== "string") return null;
  return {
    preview: value.preview,
    type: typeof value.type === "string" ? value.type : undefined,
    truncated: value.truncated === true,
    bytes: typeof value.bytes === "number" ? value.bytes : undefined,
    redacted: value.redacted === true,
  };
}

function recordedPorts(node: BlueprintGraphNode, side: "inputs" | "outputs"): Readonly<Record<string, unknown>> | null {
  const ports = node.evidence?.journal?.ports;
  if (!ports || typeof ports !== "object" || Array.isArray(ports)) return null;
  const group = (ports as Record<string, unknown>)[side];
  return group && typeof group === "object" && !Array.isArray(group) ? group as Record<string, unknown> : null;
}

/** What flowed through a data pin in the selected run; null when no run is selected or the pin carries no data. */
export function pinValue(model: BlueprintGraphModel, node: BlueprintGraphNode, pin: BlueprintPin): PinValue | null {
  if (model.mode !== "run" || pin.kind !== "data") return null;
  const recorded = recordedPorts(node, pin.side === "in" ? "inputs" : "outputs");
  const value = portValue(recorded?.[portName(pin)]);
  if (value) return { kind: "value", value };
  const summary = node.evidence?.journal?.result_summary;
  if (pin.side === "out" && typeof summary === "string" && summary.trim()) return { kind: "summary", text: summary.trim() };
  return { kind: "none" };
}

/** A data wire carries what its source pin produced. */
export function wireValue(model: BlueprintGraphModel, wire: BlueprintGraphWire): PinValue | null {
  if (wire.kind !== "data") return null;
  const source = model.nodes.find((node) => node.key === wire.from.node);
  const pin = source?.outputs.find((entry) => entry.id === wire.from.pin);
  return source && pin ? pinValue(model, source, pin) : null;
}

/** JSON reads pretty, anything else stays as recorded; long previews are cut for the popover, not for Copy. */
export function previewText(preview: string): { text: string; cut: boolean } {
  let text = preview;
  try { text = JSON.stringify(JSON.parse(preview), null, 2); } catch { text = preview; }
  return text.length > PREVIEW_LIMIT ? { text: `${text.slice(0, PREVIEW_LIMIT)}…`, cut: true } : { text, cut: false };
}
