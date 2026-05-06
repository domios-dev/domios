interface Props {
  status: "ready" | "indexing" | "error" | "loading";
  errorText?: string | null;
  zoomPercent?: number | null;
  nodeCount?: number;
  edgeCount?: number;
  truncated?: boolean;
}

const STATUS_LABEL: Record<Props["status"], string> = {
  ready: "ready",
  indexing: "indexing",
  error: "error",
  loading: "loading…",
};

const STATUS_COLOR: Record<Props["status"], string> = {
  ready: "var(--ct-fg)",
  indexing: "var(--ct-yellow)",
  error: "var(--ct-red)",
  loading: "var(--ct-muted-fg)",
};

export function BottomStrip({
  status,
  errorText,
  zoomPercent,
  nodeCount,
  edgeCount,
  truncated,
}: Props) {
  return (
    <div
      className="flex items-center gap-3 px-3 h-6 border-t border-[var(--ct-border)] bg-[var(--ct-bg)] text-[10px] text-[var(--ct-muted-fg)]"
      title={errorText ?? undefined}
    >
      <span className="flex items-center gap-1">
        <span
          className="inline-block w-1.5 h-1.5 rounded-full"
          style={{ background: STATUS_COLOR[status] }}
        />
        <span style={{ color: STATUS_COLOR[status] }}>{STATUS_LABEL[status]}</span>
      </span>
      {nodeCount != null && (
        <span>
          {nodeCount} nodes / {edgeCount ?? 0} edges
        </span>
      )}
      {truncated && <span className="text-[var(--ct-yellow)]">truncated</span>}
      {errorText && <span className="text-[var(--ct-red)] truncate">{errorText}</span>}
      <div className="flex-1" />
      {zoomPercent != null && <span>{zoomPercent}%</span>}
    </div>
  );
}
