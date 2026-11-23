import { BROWSER_SURFACE_CHROME } from "../../../../../src/components/Surface/browserChrome";
import { resolveToolbar, surfaceLabel, type SurfaceState } from "../../../../../src/components/Surface/surfaceChromeModel";
import { PointAndAsk } from "../../../../../src/components/Surface/PointAndAsk";
import { SurfaceToolbar } from "../../../../../src/components/Surface/SurfaceToolbar";
import siteDark from "../../assets/domios-site-dark.png";
import siteLight from "../../assets/domios-site-light.png";
import { Camera, easeOut, range, Reveal, useStage, type Scene } from "../../engine";

// Mira's browser in Domios opens domios.dev, then points at its headline and asks about it, on the words of the line.
const noop = () => undefined;
const ADDRESS = "domios.dev";
const TYPE = [3.0, 4.0] as const;
const LOAD = [4.1, 4.8] as const;
const PICK = 5.3;
const ASK = 6.4;
const DURATION = 8;
/** The site's headline in the canvas, measured from the screenshot at the canvas width. */
const HEADLINE = { left: 372, top: 174, width: 540, height: 90 };
const SITE_CSS = `.film-site-dark, .film-site-light { display: block; width: 100%; } [data-ct-theme="light"] .film-site-dark, [data-ct-theme="dark"] .film-site-light { display: none; }`;

function stateAt(t: number): SurfaceState {
  const typed = ADDRESS.slice(0, Math.round(range(t, TYPE[0], TYPE[1]) * ADDRESS.length));
  const loaded = t >= LOAD[1];
  return { owner: "Mira", title: loaded ? "Domios · Agents live here." : "New tab", url: typed ? `https://${typed}` : "", profile: "owner", profiles: ["owner", "clean"] };
}

function BrowserSite({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const state = stateAt(t);
  const label = surfaceLabel(state.owner, state.title ?? "");
  const picking = t >= PICK;
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking, state, busy: new Set() });
  const pick = range(t, PICK, PICK + 0.35, easeOut);
  const progress = range(t, LOAD[0], LOAD[1], easeOut);
  const page = range(t, LOAD[1] - 0.3, LOAD[1] + 0.4, easeOut);
  const surface = range(t, 0, 0.5, easeOut);
  return (
    <Camera t={t} width={1280} height={720} keys={portrait ? [
      { t: 0, x: 560, y: 360, scale: 1.5 },
      { t: PICK, x: 640, y: 330, scale: 1.6 },
      { t: DURATION, x: 640, y: 300, scale: 1.7 },
    ] : [
      { t: 0, x: 640, y: 360, scale: 1.22 },
      { t: PICK, x: 640, y: 360, scale: 1.22 },
      { t: DURATION, x: 640, y: 290, scale: 1.5 },
    ]}>
      <style>{SITE_CSS}</style>
      <section className="ct-surface" aria-label={label} style={{ position: "absolute", inset: 0, opacity: surface, transform: `scale(${0.96 + 0.04 * surface})`, borderRadius: 12, overflow: "hidden", border: "1px solid var(--ct-border)", boxShadow: "var(--ct-shadow-lg)" }}>
        <SurfaceToolbar label={label} items={items} onButton={noop} onToggle={noop} onSubmit={noop} onSelect={noop} />
        <div className="ct-surface-stage">
          <div className="ct-surface-canvas" data-mode={picking ? "picking" : "interactive"} aria-label={state.title} style={{ position: "relative", overflow: "hidden", background: "var(--ct-bg)" }}>
            {page < 1 && <div style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center", color: "var(--ct-muted-fg)", fontSize: 22, opacity: 0.6 * (1 - page) }}>New tab</div>}
            {progress > 0 && progress < 1 && <div style={{ position: "absolute", left: 0, top: 0, height: 3, width: `${progress * 100}%`, background: "var(--ct-accent)", zIndex: 1 }} />}
            {page > 0 && (
              <div style={{ position: "absolute", inset: 0, opacity: page, transform: `translateY(${(1 - page) * 10}px)` }}>
                <img className="film-site-dark" src={siteDark} alt="" />
                <img className="film-site-light" src={siteLight} alt="" />
              </div>
            )}
            {pick > 0 && <div style={{ position: "absolute", ...HEADLINE, borderRadius: 6, outline: "2px solid var(--ct-accent)", outlineOffset: 4, background: "color-mix(in srgb, var(--ct-accent) 10%, transparent)", opacity: pick }} />}
          </div>
          <Reveal t={t} at={ASK} dur={0.35} dy={14}>
            <PointAndAsk
              owner={state.owner}
              subject={{ kind: "element", card: { kind: "text", name: "Heading \"Agents live here.\"", path: "domios.dev · main > h1" } }}
              position={{ left: HEADLINE.left, top: HEADLINE.top + HEADLINE.height + 56 }}
              onSend={noop}
              onCancel={noop}
            />
          </Reveal>
        </div>
      </section>
    </Camera>
  );
}

const browserSite: Scene = {
  id: "browser-site",
  duration: DURATION,
  render: (t) => <BrowserSite t={t} />,
};

export default browserSite;
