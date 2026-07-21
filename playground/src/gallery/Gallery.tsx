import { useEffect, useMemo, useRef, useState } from "react";
import { AnimationInspector } from "./AnimationInspector";
import type { Specimen } from "./types";

const modules = import.meta.glob("./specimens/*.spec.tsx", { eager: true }) as Record<string, { default?: Specimen }>;

export const specimens = Object.values(modules)
  .map((module) => module.default)
  .filter((specimen): specimen is Specimen => Boolean(specimen))
  .sort((a, b) => a.title.localeCompare(b.title));

const CATEGORY_ORDER = ["Buttons", "Controls", "Status", "Sidebar"];

function GalleryShell({ activeId, children }: { activeId?: string; children: React.ReactNode }) {
  const groups = CATEGORY_ORDER.map((category) => ({ category, entries: specimens.filter((specimen) => specimen.category === category) })).filter((group) => group.entries.length);
  return <div className="gallery-catalog">
    <aside className="gallery-nav" aria-label="Component catalog">
      <a className="gallery-nav-title" href="#gallery">Component gallery</a>
      {groups.map(({ category, entries }) => <section key={category} className="gallery-nav-group" aria-labelledby={`gallery-category-${category}`}>
        <h2 id={`gallery-category-${category}`}>{category}</h2>
        {entries.map((specimen) => <a key={specimen.id} className={activeId === specimen.id ? "is-active" : ""} href={`#gallery/${specimen.id}`} aria-current={activeId === specimen.id ? "page" : undefined}>{specimen.title}</a>)}
      </section>)}
    </aside>
    <div className="gallery-content">{children}</div>
  </div>;
}

export function GalleryIndex() {
  if (!specimens.length) {
    return <GalleryShell><main className="portal-inner gallery-empty"><p className="portal-kicker">Component Gallery</p><h1>No specimens yet</h1><p className="portal-lede">Add a <code>*.spec.tsx</code> file in <code>gallery/specimens</code> to populate this space.</p></main></GalleryShell>;
  }
  return <GalleryShell><main className="portal-inner">
      <p className="portal-kicker">Component Gallery</p>
      <h1>Motion specimens</h1>
      <p className="portal-lede">Live components, their intentional states, and a frame-by-frame view of the browser motion underneath.</p>
      <p className="portal-lede">Choose a component from the catalog to inspect every intentional state together.</p>
    </main></GalleryShell>;
}

export function GalleryDetail({ id }: { id: string }) {
  const specimen = useMemo(() => specimens.find((entry) => entry.id === id), [id]);
  const [stateId, setStateId] = useState(() => specimen?.states[0]?.id ?? "");
  const stage = useRef<HTMLDivElement>(null);

  useEffect(() => setStateId(specimen?.states[0]?.id ?? ""), [specimen?.id]);

  if (!specimen) return <GalleryShell><main className="portal-inner gallery-empty"><p className="portal-kicker">Component Gallery</p><h1>Specimen not found</h1><a className="portal-back" href="#gallery">← gallery</a></main></GalleryShell>;
  const state = specimen.states.find((entry) => entry.id === stateId) ?? specimen.states[0];
  if (!state) return <GalleryShell activeId={id}><main className="portal-inner gallery-empty"><h1>{specimen.title}</h1><p className="portal-lede">This specimen has no states.</p></main></GalleryShell>;
  const Component = specimen.Component;
  return <GalleryShell activeId={id}><main className="portal-inner gallery-detail">
      <p className="portal-kicker">{specimen.where}</p>
      <h1>{specimen.title}</h1>
      {specimen.notes && <p className="portal-lede">{specimen.notes}</p>}
      <section className="gallery-state-board" aria-label={`${specimen.title} states`}>
        {specimen.states.map((entry) => <button key={entry.id} className={`gallery-state-stage${entry.id === state.id ? " is-focused" : ""}`} type="button" onClick={() => setStateId(entry.id)} aria-pressed={entry.id === state.id}>
          <span className="gallery-state-label">{entry.label}</span>
          <span className="gallery-stage-instance" ref={entry.id === state.id ? stage : undefined}><Component {...entry.props} /></span>
        </button>)}
      </section>
      <AnimationInspector specimen={specimen} state={state} stage={stage} />
    </main></GalleryShell>;
}
