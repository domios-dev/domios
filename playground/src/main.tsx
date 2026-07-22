import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { LiquidGlassLab } from "./concepts/liquidglass/LiquidGlassLab";
import { GalleryDetail, GalleryIndex } from "./gallery/Gallery";
import "./ds/tokens.css";
import "./playground.css";
import "./portal.css";

const LABS = [
  {
    id: "liquid-glass",
    tag: "react lab",
    title: "Living Controls",
    blurb: "Real buttons, dots and toggles with liquid squash-jump motion: hover bounce, wall-bouncing thumbs, the living dots.",
    render: () => <LiquidGlassLab />,
  },
] as const;

const PAGES = [
  {
    href: "liquidglass-studies.html",
    tag: "standalone svg",
    title: "SVG Glass Studies",
    blurb: "Eight looping micro-studies of the liquid-glass motion style — pure SVG filters, opens as its own page.",
  },
] as const;

function readHash(): string {
  return window.location.hash.replace(/^#/, "");
}

function Portal() {
  const [route, setRoute] = useState(readHash);

  useEffect(() => {
    const onHash = () => setRoute(readHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const lab = LABS.find((l) => l.id === route);
  if (lab) {
    return (
      <div className="portal">
        <a className="portal-back" href="#">← all experiments</a>
        {lab.render()}
      </div>
    );
  }

  if (route === "gallery") {
    return <div className="portal"><GalleryIndex /></div>;
  }

  if (route.startsWith("gallery/")) {
    return <div className="portal"><GalleryDetail id={route.slice("gallery/".length)} /></div>;
  }

  return (
    <div className="portal">
      <main className="portal-inner">
        <p className="portal-kicker">CodeTerm · internal</p>
        <h1>Motion Playground</h1>
        <p className="portal-lede">
          Every motion-design experiment in one place. The direction: liquid squash-jump —
          the physics of the jumping dots applied as a fluid, restrained motion language
          across the real interface.
        </p>
        <div className="portal-grid">
          {LABS.map((l) => (
            <a key={l.id} className="portal-card" href={`#${l.id}`}>
              <span className="portal-card-tag">{l.tag}</span>
              <h2>{l.title}</h2>
              <p>{l.blurb}</p>
            </a>
          ))}
          <a className="portal-card" href="#gallery">
            <span className="portal-card-tag">component archive</span>
            <h2>Component Gallery</h2>
            <p>Inspectable motion specimens with state controls and a frame-by-frame filmstrip.</p>
          </a>
          {PAGES.map((p) => (
            <a key={p.href} className="portal-card" href={p.href}>
              <span className="portal-card-tag">{p.tag}</span>
              <h2>{p.title}</h2>
              <p>{p.blurb}</p>
            </a>
          ))}
        </div>
      </main>
    </div>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Playground root is missing");

createRoot(root).render(
  <StrictMode>
    <Portal />
  </StrictMode>,
);
