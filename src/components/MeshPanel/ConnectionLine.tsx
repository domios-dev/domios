interface Props {
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
  pushCount: number;
  pulsing: boolean;
  isGhost?: boolean;
}

const BASE_WIDTH = 1;
const MAX_EXTRA_WIDTH = 4;
const VOLUME_CAP = 10;

export function ConnectionLine({ fromX, fromY, toX, toY, pushCount, pulsing, isGhost }: Props) {
  const intensity = Math.min(pushCount, VOLUME_CAP) / VOLUME_CAP;
  const width = BASE_WIDTH + MAX_EXTRA_WIDTH * intensity;
  const opacity = isGhost ? 0.25 : 0.45 + 0.45 * intensity;
  const stroke = intensity > 0 ? "var(--ct-accent)" : "var(--ct-border)";

  return (
    <>
      <line
        x1={fromX}
        y1={fromY}
        x2={toX}
        y2={toY}
        stroke={stroke}
        strokeWidth={width}
        strokeOpacity={opacity}
        strokeLinecap="round"
        strokeDasharray={isGhost ? "6 4" : undefined}
      />
      {pulsing && !isGhost && (
        <>
          <circle r={Math.max(3, width + 1)} fill="var(--ct-accent)">
            <animateMotion
              dur="1.5s"
              repeatCount="indefinite"
              path={`M${fromX},${fromY} L${toX},${toY}`}
            />
          </circle>
          <circle r={Math.max(3, width + 1)} fill="var(--ct-accent)" opacity="0.7">
            <animateMotion
              dur="1.5s"
              repeatCount="indefinite"
              path={`M${toX},${toY} L${fromX},${fromY}`}
            />
          </circle>
        </>
      )}
    </>
  );
}
