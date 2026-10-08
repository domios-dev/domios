import { liquidSignal } from "./liquidSignal";

export function BlueprintLiquidSignal({ path, progress, to }: { path: string; progress: number; to: { x: number; y: number } }) {
  const signal = liquidSignal(progress);
  return <g data-replay-motion="bead">
    {[3, 2, 1, 0].map((lag) => {
      const pose = liquidSignal(Math.max(0, progress - lag * 0.035));
      return <g key={lag} className="bp-bead" style={{ offsetPath: `path('${path}')`, offsetDistance: `${pose.distance * 100}%`, opacity: lag ? pose.opacity * (0.16 - lag * 0.035) * (1 - signal.landing) : signal.opacity }}>
        <g transform={`scale(${pose.scaleX} ${pose.scaleY})`}>
          <ellipse rx="7" ry="4" />
          {lag === 0 && <ellipse className="bp-bead__glint" cx="-1.5" cy="-1.2" rx="2" ry="0.8" />}
        </g>
      </g>;
    })}
    {signal.landing > 0 && <circle className="bp-landing-ripple" cx={to.x} cy={to.y} r={4 + signal.landing * 10} opacity={(1 - signal.landing) * 0.35} />}
  </g>;
}
