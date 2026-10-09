import { useMemo, useState, type ReactNode } from "react";
import { PortalHostContext } from "../mocks/portalHost";
import type { Specimen, SpecimenState } from "./types";

const modules = import.meta.glob("./specimens/*.spec.tsx", { eager: true }) as Record<string, { default?: Specimen }>;

export const specimens = Object.values(modules)
  .map((module) => module.default)
  .filter((specimen): specimen is Specimen => Boolean(specimen))
  .sort((a, b) => a.title.localeCompare(b.title));

const CATEGORY_ORDER = ["Buttons", "Controls", "Menus", "Composer", "Voice", "Status", "Sidebar"];

function GalleryShell({ activeId, children }: { activeId?: string; children: ReactNode }) {
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
      <h1>Product components</h1>
      <p className="portal-lede">Every stage renders the shipped component from <code>src/</code> with the shipped stylesheet, fed by fixture data, so the catalog cannot drift from the app.</p>
      <p className="portal-lede">Choose a component from the catalog to see every meaningful state side by side.</p>
    </main></GalleryShell>;
}

function StateStage({ state }: { state: SpecimenState }) {
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  return <figure className="gallery-state-stage" data-state-id={state.id}>
    <figcaption className="gallery-state-label">{state.label}</figcaption>
    <div className="gallery-stage-instance">
      {host && <PortalHostContext.Provider value={host}>{state.render()}</PortalHostContext.Provider>}
      <div ref={setHost} className="gallery-portal-host" />
    </div>
  </figure>;
}

export function GalleryDetail({ id }: { id: string }) {
  const specimen = useMemo(() => specimens.find((entry) => entry.id === id), [id]);
  if (!specimen) return <GalleryShell><main className="portal-inner gallery-empty"><p className="portal-kicker">Component Gallery</p><h1>Specimen not found</h1><a className="portal-back" href="#gallery">← gallery</a></main></GalleryShell>;
  return <GalleryShell activeId={id}><main className="portal-inner gallery-detail" key={specimen.id}>
      <p className="portal-kicker">{specimen.where}</p>
      <h1>{specimen.title}</h1>
      {specimen.notes && <p className="portal-lede">{specimen.notes}</p>}
      <section className={`gallery-state-board${specimen.stage === "wide" ? " gallery-state-board--wide" : ""}`} aria-label={`${specimen.title} states`}>
        {specimen.states.map((state) => <StateStage key={state.id} state={state} />)}
      </section>
    </main></GalleryShell>;
}
