import { Hammer } from "lucide-react";
import { useRef } from "react";
import { useHammerStrike } from "./useHammerStrike";

export function BuildShellIcon({ running }: { running: boolean }) {
  const hammerRef = useRef<SVGGElement>(null);
  const sparksRef = useRef<SVGGElement>(null);
  useHammerStrike(hammerRef, sparksRef, running);
  return (
    <svg role="img" aria-label="build shell" width={14} height={14} viewBox="0 0 24 24">
      <g transform="translate(1.2 1.2) scale(0.9)">
        <g ref={hammerRef} style={{ transformBox: "view-box", transformOrigin: "9px 15px" }}>
          <Hammer size={24} aria-hidden="true" />
        </g>
      </g>
      <g ref={sparksRef} aria-hidden="true" pointerEvents="none" fill="var(--ct-accent)" stroke="none">
        {[0, 1, 2].map((spark) => <circle key={spark} cx={19} cy={18} r={0.9} opacity={0} />)}
      </g>
    </svg>
  );
}
