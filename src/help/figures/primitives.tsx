export const stroke = "var(--ct-border)";
export const accent = "var(--ct-accent)";
export const fg = "var(--ct-fg)";
export const muted = "var(--ct-muted-fg)";
export const fill = "color-mix(in srgb, var(--ct-accent) 12%, var(--ct-bg))";

export function Box({
  x,
  y,
  w,
  h,
  label,
  sub,
  emphasis,
  dashed,
}: {
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  sub?: string;
  emphasis?: boolean;
  dashed?: boolean;
}) {
  return (
    <g>
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={8}
        fill={emphasis ? fill : "var(--ct-bg)"}
        stroke={emphasis || dashed ? accent : stroke}
        strokeWidth={1.5}
        strokeDasharray={dashed ? "4 3" : undefined}
      />
      <text x={x + w / 2} y={y + (sub ? 22 : 28)} textAnchor="middle" fill={fg} fontSize={12} fontWeight={600}>
        {label}
      </text>
      {sub && (
        <text x={x + w / 2} y={y + 38} textAnchor="middle" fill={muted} fontSize={10}>
          {sub}
        </text>
      )}
    </g>
  );
}

export function ArrowMarker({ id }: { id: string }) {
  return (
    <marker id={id} markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
      <path d="M0,0 L6,3 L0,6 Z" fill={accent} />
    </marker>
  );
}

export function Arrow({
  x1,
  y1,
  x2,
  y2,
  marker = "help-arrow",
  dashed,
}: {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  marker?: string;
  dashed?: boolean;
}) {
  return (
    <line
      x1={x1}
      y1={y1}
      x2={x2}
      y2={y2}
      stroke={accent}
      strokeWidth={1.5}
      strokeDasharray={dashed ? "4 3" : undefined}
      markerEnd={`url(#${marker})`}
    />
  );
}

export function Caption({ x, y, text }: { x: number; y: number; text: string }) {
  return (
    <text x={x} y={y} textAnchor="middle" fill={muted} fontSize={9}>
      {text}
    </text>
  );
}
