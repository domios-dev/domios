// Pure classification + serialization for pty-write-to-render diagnostics; no
// page/api/io so it is unit-testable and safe to import from the metrics sink.

export interface ViewportGeometry {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  bottomDelta: number;
  anchored: boolean;
}

export interface FocusViewState {
  rootMounted: boolean;
  paneVisible: boolean;
  tabActive: boolean;
  terminalMode: boolean;
}

export interface SampleDiagnostic {
  index: number;
  marker: string;
  tOutputMs: number | null;
  tRenderMs: number | null;
  budgetMs: number;
  loadavg: [number, number, number];
  focus: FocusViewState;
  viewport: ViewportGeometry | null;
}

export type DiagnosticPhase = "output-absent" | "focus-view-bottom-mismatch" | "render-pipeline" | "render-late" | "ok";
export type DiagnosticVerdict = "(a)" | "(b)" | "(c)" | "ok";

export interface DiagnosticClassification {
  phase: DiagnosticPhase;
  verdict: DiagnosticVerdict;
  detail: string;
}

export const BOTTOM_ANCHOR_THRESHOLD_PX = 2;

export function computeAnchored(scrollTop: number, scrollHeight: number, clientHeight: number, thresholdPx = BOTTOM_ANCHOR_THRESHOLD_PX): { bottomDelta: number; anchored: boolean } {
  const bottomDelta = scrollHeight - clientHeight - scrollTop;
  return { bottomDelta, anchored: scrollHeight <= clientHeight || bottomDelta <= thresholdPx };
}

function focusMismatches(focus: FocusViewState, viewport: ViewportGeometry | null): string[] {
  const out: string[] = [];
  if (!focus.rootMounted) out.push("root-unmounted");
  if (!focus.paneVisible) out.push("pane-hidden");
  if (!focus.tabActive) out.push("tab-inactive");
  if (!focus.terminalMode) out.push("not-terminal-mode");
  if (viewport && !viewport.anchored) out.push(`not-bottom-anchored(delta=${viewport.bottomDelta})`);
  return out;
}

export function classifyDiagnostic(sample: SampleDiagnostic): DiagnosticClassification {
  const renderedInBudget = sample.tRenderMs !== null && sample.tRenderMs <= sample.budgetMs;
  if (renderedInBudget) return { phase: "ok", verdict: "ok", detail: `render@${sample.tRenderMs}ms` };

  if (sample.tOutputMs === null) {
    return { phase: "output-absent", verdict: "(b)", detail: "output-absent" };
  }

  if (sample.tRenderMs !== null) {
    return { phase: "render-late", verdict: "(c)", detail: `output@${sample.tOutputMs}ms render@${sample.tRenderMs}ms>budget${sample.budgetMs}ms` };
  }

  const mismatches = focusMismatches(sample.focus, sample.viewport);
  if (mismatches.length > 0) {
    return { phase: "focus-view-bottom-mismatch", verdict: "(a)", detail: `output@${sample.tOutputMs}ms render-absent; ${mismatches.join(",")}` };
  }

  return { phase: "render-pipeline", verdict: "(a)", detail: `output@${sample.tOutputMs}ms render>${sample.budgetMs}ms` };
}

// Short phase+values string for the fail-loud assertion message.
export function formatDiagnosticMessage(sample: SampleDiagnostic): string {
  const c = classifyDiagnostic(sample);
  return `${c.verdict} ${c.phase}: ${c.detail}`;
}

export type DiagnosticRow = {
  type: "diagnostic";
  name: string;
  index: number;
  marker: string;
  t_output_ms: number | null;
  t_render_ms: number | null;
  budget: number;
  loadavg: [number, number, number];
  phase: DiagnosticPhase;
  verdict: DiagnosticVerdict;
  detail: string;
  focus: FocusViewState;
  viewport: ViewportGeometry | null;
  mode?: string;
} & Record<string, unknown>;

export function diagnosticRow(name: string, sample: SampleDiagnostic, mode?: string): DiagnosticRow {
  const c = classifyDiagnostic(sample);
  return {
    type: "diagnostic",
    name,
    index: sample.index,
    marker: sample.marker,
    t_output_ms: sample.tOutputMs,
    t_render_ms: sample.tRenderMs,
    budget: sample.budgetMs,
    loadavg: sample.loadavg,
    phase: c.phase,
    verdict: c.verdict,
    detail: c.detail,
    focus: sample.focus,
    viewport: sample.viewport,
    ...(mode === undefined ? {} : { mode }),
  };
}
