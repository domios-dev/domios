import type { CSSProperties } from "react";
import type { SurfaceState } from "../../../../src/components/Surface/surfaceChromeModel";

export const BROWSER_STATE: SurfaceState = {
  owner: "Mira",
  title: "Blueprints · Domios docs",
  url: "http://127.0.0.1:4321/docs/blueprints#schedule",
  profile: "owner",
  profiles: ["owner", "clean"],
};

export const PICKED_COMMAND = "domios blueprint apply release-gate \\\n  --set lane=lane/offline-sync \\\n  --note \"2.4 gate\"";

const mono = '"JetBrains Mono", ui-monospace, monospace';
const muted: CSSProperties = { color: "var(--ct-muted-fg, var(--ct-muted))" };
const NAV = ["Getting started", "Tabs and Teams", "Plans", "Blueprints", "Crons", "Mesh", "Memory", "Plugins"];
const card: CSSProperties = { padding: "14px 18px", borderRadius: 8, background: "var(--ct-surface)", border: "1px solid var(--ct-border)" };

/** A local docs page as the browser surface shows it: static markup on --ct-* tokens. */
export function DocsPage({ picking }: { picking: boolean }) {
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)", fontSize: 17, lineHeight: 1.6, overflow: "hidden" }}>
    <header style={{ display: "flex", alignItems: "center", gap: 28, height: 56, padding: "0 32px", borderBottom: "1px solid var(--ct-border)", background: "var(--ct-sidebar-bg)" }}>
      <strong style={{ fontSize: 17 }}>Domios docs</strong>
      {["Guide", "CLI", "Blueprints", "Changelog"].map((link) => <span key={link} style={{ ...muted, fontSize: 15 }}>{link}</span>)}
      <span style={{ marginLeft: "auto", ...muted, fontSize: 14, fontFamily: mono }}>v0.42</span>
    </header>
    <div style={{ display: "grid", gridTemplateColumns: "210px 1fr", height: "calc(100% - 56px)" }}>
      <nav style={{ padding: "26px 22px", borderRight: "1px solid var(--ct-border)", display: "flex", flexDirection: "column", gap: 8, fontSize: 15 }}>
        {NAV.map((item) => <span key={item} style={item === "Blueprints" ? { color: "var(--ct-accent)", fontWeight: 600 } : muted}>{item}</span>)}
      </nav>
      <main style={{ padding: "30px 52px", maxWidth: 900 }}>
        <h1 style={{ fontSize: 34, fontWeight: 650, margin: "0 0 8px" }}>Blueprints</h1>
        <p style={{ ...muted, margin: "0 0 26px" }}>A Blueprint is a graph of probes, agents and actions that runs on its own and reports to its owner tab or Team.</p>
        <h2 style={{ fontSize: 25, fontWeight: 620, margin: "0 0 10px" }}>Schedule a Blueprint</h2>
        <p style={{ margin: "0 0 16px" }}>Apply a built-in recipe and bind it to a lane. The release gate re-runs whenever the lane merges.</p>
        <div style={{ ...card, borderLeft: "3px solid var(--ct-accent)", fontSize: 16, marginBottom: 20 }}>
          Re-applying a name retargets it. Check a custom graph with <code style={{ fontFamily: mono }}>domios blueprint check</code> first.
        </div>
        <pre style={{ ...card, margin: 0, fontFamily: mono, fontSize: 15, whiteSpace: "pre-wrap", outline: picking ? "2px solid var(--ct-accent)" : "none", outlineOffset: 4 }}>
          {PICKED_COMMAND}
        </pre>
      </main>
    </div>
  </div>;
}
