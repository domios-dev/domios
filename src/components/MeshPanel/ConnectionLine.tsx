interface Props {
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
  active: boolean;
  pulsing: boolean;
}

export function ConnectionLine({ fromX, fromY, toX, toY, active, pulsing }: Props) {
  return (
    <>
      <line
        x1={fromX}
        y1={fromY}
        x2={toX}
        y2={toY}
        stroke={active ? "var(--ct-accent)" : "var(--ct-border)"}
        strokeWidth={active ? 1.5 : 0.5}
        strokeOpacity={active ? 0.8 : 0.4}
        strokeDasharray={active ? "" : "4 3"}
      />
      {pulsing && (
        <circle r="3" fill="var(--ct-accent)">
          <animateMotion
            dur="2s"
            repeatCount="indefinite"
            path={`M${fromX},${fromY} L${toX},${toY}`}
          />
        </circle>
      )}
    </>
  );
}
