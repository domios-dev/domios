export type PerfEventPhase = "sent" | "output" | "output-error" | "render" | "capture";

export interface PerfEventRow {
  run: string;
  test: string;
  index: number;
  seq: number;
  phase: PerfEventPhase;
  t_ms: number;
  [key: string]: unknown;
}

export interface PerfEventBounceOptions {
  run: string;
  test: string;
  index: number;
  startedAt: number;
}

export type PerfEventAppender = (row: PerfEventRow) => void;

type FocusState = {
  rootMounted?: boolean;
  tabVisible?: boolean;
  tabActive?: boolean;
  terminalMode?: boolean;
};

type ViewportState = { anchored?: boolean; bottomDelta?: number };

export class PerfEventBounce {
  private readonly phases = new Set<PerfEventPhase>();
  private readonly rows = new Map<PerfEventPhase, PerfEventRow>();
  private seq = 0;

  constructor(
    private readonly options: PerfEventBounceOptions,
    private readonly append: PerfEventAppender,
    private readonly now: () => number = () => performance.now(),
  ) {}

  record(phase: PerfEventPhase, details: Record<string, unknown> = {}, tMsOverride?: number): PerfEventRow | null {
    if (this.phases.has(phase)) return null;
    this.phases.add(phase);
    const row: PerfEventRow = {
      run: this.options.run,
      test: this.options.test,
      index: this.options.index,
      seq: this.seq++,
      phase,
      t_ms: tMsOverride ?? Math.max(0, this.now() - this.options.startedAt),
      ...details,
    };
    this.rows.set(phase, row);
    this.append(row);
    return row;
  }

  recorded(phase: PerfEventPhase): boolean {
    return this.phases.has(phase);
  }

  phase(phase: PerfEventPhase): PerfEventRow | undefined {
    return this.rows.get(phase);
  }

  elapsedMs(): number {
    return Math.max(0, this.now() - this.options.startedAt);
  }
}

function outputErrorContext(row: PerfEventRow | undefined): string {
  return typeof row?.error === "string" ? `; output-error: ${row.error}` : "";
}

function viewMismatches(focus: FocusState | undefined, viewport: ViewportState | undefined): string[] {
  const mismatches: string[] = [];
  if (!focus?.rootMounted) mismatches.push("root-unmounted");
  if (!focus?.tabVisible) mismatches.push("tab-hidden");
  if (!focus?.tabActive) mismatches.push("tab-inactive");
  if (!focus?.terminalMode) mismatches.push("not-terminal-mode");
  if (!viewport?.anchored) mismatches.push(`bottomDelta=${viewport?.bottomDelta ?? "unknown"}`);
  return mismatches;
}

export function formatPerfFailureState(bounce: PerfEventBounce, budgetMs: number): string {
  const output = bounce.phase("output");
  const outputError = bounce.phase("output-error");
  const render = bounce.phase("render");
  const capture = bounce.phase("capture");
  if (!output) {
    if (typeof outputError?.error === "string") return `(b) output-error: ${outputError.error} render>${budgetMs}ms`;
    return `(b) output-absent: render>${budgetMs}ms`;
  }
  if (render?.late === true) return `(c) render-late: output@${output.t_ms}ms render@${render.t_ms}ms>budget${budgetMs}ms${outputErrorContext(outputError)}`;
  const mismatches = viewMismatches(capture?.focus as FocusState | undefined, capture?.viewport as ViewportState | undefined);
  if (mismatches.length > 0) return `(a) focus-view-bottom-mismatch: output@${output.t_ms}ms render>${budgetMs}ms; ${mismatches.join(",")}${outputErrorContext(outputError)}`;
  return `(a) render-pipeline: output@${output.t_ms}ms render>${budgetMs}ms${outputErrorContext(outputError)}`;
}
