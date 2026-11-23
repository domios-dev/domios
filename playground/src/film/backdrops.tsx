import type { ReactNode } from "react";
import { range } from "./engine";
import { DotField } from "./scenes/mesh/brandField";

export interface BackdropProps {
  t: number;
  width: number;
  height: number;
  /** Seconds at which the reel hard-cuts; backdrops may answer them with a quiet pulse. */
  pulses: number[];
  /** 0..1 overall strength, from `?bdi=`. */
  intensity: number;
}

const layer = (children: ReactNode) => <div style={{ position: "absolute", inset: 0, color: "var(--ct-accent)" }}>{children}</div>;

/** Lies under every scene; scenes that paint their own full background cover it. Chosen with `?backdrop=<id>`. */
export const BACKDROPS: Record<string, (p: BackdropProps) => ReactNode> = {
  none: () => null,
  dots: ({ t, width, height, pulses, intensity }) =>
    layer(<DotField t={t} releases={pulses} intensity={1.1 * intensity} restIntensity={4.5 * intensity} frameWidth={width} frameHeight={height} />),
  "dots-still": ({ t, width, height, intensity }) =>
    layer(<DotField t={t} releases={[]} intensity={intensity} restIntensity={4.5 * intensity} frameWidth={width} frameHeight={height} />),
  glow: ({ t, width, height, pulses, intensity }) => {
    const last = pulses.filter((p) => p <= t).pop();
    const kick = last === undefined ? 0 : 1 - range(t, last, last + 1.2);
    const x = 50 + 18 * Math.sin(t / 7);
    const y = 45 + 12 * Math.cos(t / 9);
    const a = Math.round((7 + 6 * kick) * intensity);
    return (
      <div
        style={{
          position: "absolute",
          inset: 0,
          width,
          height,
          background: `radial-gradient(ellipse 60% 55% at ${x}% ${y}%, color-mix(in srgb, var(--ct-accent) ${a}%, transparent), transparent 70%)`,
        }}
      />
    );
  },
};

export const DEFAULT_BACKDROP = "dots";
