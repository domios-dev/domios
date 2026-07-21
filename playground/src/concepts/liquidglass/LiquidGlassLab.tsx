import { useState } from "react";
import { GlassButton } from "./GlassButton";
import { LivingDots } from "./LivingDots";
import { LiquidFlip, LiquidSegmented, LiquidToggle } from "./LiquidControls";
import "./liquidglass.css";

const DENSITY = ["Compact", "Cozy", "Roomy"];
const SCOPE = ["Local", "Mesh"];

// Small labelled bay so each specimen reads as a deliberate exhibit, not a demo
// dump. Layout uses reused Tailwind utilities (proves Tailwind is wired here).
function Bay({ eyebrow, title, children }: { eyebrow: string; title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-4 rounded-xl border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-surface)_55%,transparent)] p-6 backdrop-blur-md">
      <header className="flex flex-col gap-1">
        <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--ct-muted-fg)]">{eyebrow}</span>
        <h2 className="text-base font-medium text-[var(--ct-fg)]">{title}</h2>
      </header>
      {children}
    </section>
  );
}

export function LiquidGlassLab() {
  const [density, setDensity] = useState(1);
  const [scope, setScope] = useState(0);
  const [live, setLive] = useState(true);
  const [autopilot, setAutopilot] = useState(false);

  return (
    <main className="lg-lab mx-auto flex max-w-[960px] flex-col gap-8 px-6 py-12">
      <header className="flex flex-col gap-2">
        <span className="text-[11px] uppercase tracking-[0.2em] text-[var(--ct-muted-fg)]">
          CodeTerm · liquid glass × squash-jump
        </span>
        <h1 className="text-2xl font-semibold text-[var(--ct-fg)]">Living Controls</h1>
        <p className="max-w-[60ch] text-sm leading-relaxed text-[var(--ct-muted-fg)]">
          The real buttons, dots and toggles — with a translucent glass material that responds to touch
          and the squash-and-stretch physics of the jumping dots. Hover, press, focus, and switch to feel it.
        </p>
      </header>

      <Bay eyebrow="press" title="Buttons — a light hover bounce, squash-jump press, focus ring">
        <div className="flex flex-wrap items-center gap-3">
          <GlassButton variant="primary">Run agent</GlassButton>
          <GlassButton variant="accent">
            <LivingDots variant="signature" label="working" />
            <span>Working</span>
          </GlassButton>
          <GlassButton variant="ghost">Details</GlassButton>
          <GlassButton variant="danger">Stop</GlassButton>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <GlassButton variant="primary" size="lg">Large</GlassButton>
          <GlassButton variant="primary" size="sm">Small</GlassButton>
        </div>
        <p className="text-xs text-[var(--ct-muted-fg)]">
          Hover swells with one soft wobble, the label rides a beat behind; press squashes and pops. Tab for the focus ring.
        </p>
      </Bay>

      <Bay eyebrow="pulse" title="Living dots — the signature">
        <div className="flex flex-wrap items-center gap-8">
          <div className="flex items-center gap-2.5">
            <LivingDots variant="signature" />
            <span className="text-sm text-[var(--ct-muted-fg)]">working</span>
          </div>
          <div className="flex items-center gap-2.5">
            <LivingDots variant="thinking" label="thinking" />
            <span className="text-sm text-[var(--ct-muted-fg)]">thinking</span>
          </div>
          <div className="flex items-center gap-2.5">
            <LivingDots variant="signature" idle label="idle" />
            <span className="text-sm text-[var(--ct-muted-fg)]">idle</span>
          </div>
        </div>
      </Bay>

      <Bay eyebrow="switch" title="Liquid controls — thumbs bounce off the walls, never past them">
        <div className="flex flex-wrap items-center gap-x-10 gap-y-6">
          <div className="flex flex-col gap-2">
            <span className="text-xs text-[var(--ct-muted-fg)]">Density</span>
            <LiquidSegmented options={DENSITY} value={density} onChange={setDensity} ariaLabel="Density" />
          </div>
          <div className="flex flex-col gap-2">
            <span className="text-xs text-[var(--ct-muted-fg)]">Scope</span>
            <LiquidSegmented options={SCOPE} value={scope} onChange={setScope} ariaLabel="Scope" />
          </div>
          <div className="flex flex-col gap-2">
            <span className="text-xs text-[var(--ct-muted-fg)]">Live tail</span>
            <LiquidToggle checked={live} onChange={setLive} ariaLabel="Live tail" />
          </div>
          <div className="flex flex-col gap-2">
            <span className="text-xs text-[var(--ct-muted-fg)]">Autopilot</span>
            <LiquidFlip checked={autopilot} onChange={setAutopilot} ariaLabel="Autopilot" />
          </div>
        </div>
      </Bay>
    </main>
  );
}
