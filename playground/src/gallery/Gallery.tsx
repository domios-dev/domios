import { useState, type ReactNode } from "react";
import { PortalHostContext } from "../mocks/portalHost";
import { useGalleryNavigation } from "./navigation";
import { groupSpecimens } from "./navigationModel";
import type { Specimen, SpecimenState } from "./types";

const modules = import.meta.glob("./specimens/*.spec.tsx", { eager: true }) as Record<string, { default?: Specimen }>;

export const specimens = Object.values(modules)
  .map((module) => module.default)
  .filter((specimen): specimen is Specimen => Boolean(specimen))
  .sort((a, b) => a.title.localeCompare(b.title));

function GalleryShell({ activeId, children }: { activeId?: string; children: ReactNode }) {
  const { query, setQuery, searchRef, groups, previous, next } = useGalleryNavigation(specimens, activeId);
  return <div className="gallery-catalog">
    <aside className="gallery-nav" aria-label="Component catalog">
      <a className="gallery-nav-title" href="#gallery">Domios Design</a>
      <input ref={searchRef} className="gallery-search" type="search" aria-label="Search components" placeholder="Search components  /" value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => {
        if (event.code === "Escape") { event.preventDefault(); setQuery(""); }
        if (event.code === "Enter" && groups[0]?.entries[0]) { event.preventDefault(); window.location.hash = `gallery/${groups[0].entries[0].id}`; }
      }} />
      {groups.map(({ category, entries }) => <details open key={category} className="gallery-nav-group">
        <summary>{category}<span>{entries.length}</span></summary>
        {entries.map((specimen) => <a key={specimen.id} className={activeId === specimen.id ? "is-active" : ""} href={`#gallery/${specimen.id}`} aria-current={activeId === specimen.id ? "page" : undefined}>{specimen.title}</a>)}
      </details>)}
      {!groups.length && <p className="gallery-nav-empty" role="status">No components found</p>}
    </aside>
    <div className="gallery-content">
      {children}
      {activeId && <nav className="gallery-pagination" aria-label="Adjacent components">
        {previous ? <a className="ct-bar-pill" href={`#gallery/${previous.id}`} aria-keyshortcuts="[">← {previous.title}</a> : <span />}
        {next && <a className="ct-bar-pill" href={`#gallery/${next.id}`} aria-keyshortcuts="]">{next.title} →</a>}
      </nav>}
    </div>
  </div>;
}

function GalleryHeader({ title, where, notes }: { title: string; where: string; notes?: string }) {
  return <header className="gallery-header">
    <div className="gallery-heading"><h1>{title}</h1><p className="gallery-kicker">{where}</p></div>
    {notes && <details className="gallery-about"><summary className="ct-bar-pill">About</summary><p>{notes}</p></details>}
  </header>;
}

export function GalleryIndex() {
  return <GalleryShell><main className="gallery-page">
    <GalleryHeader title="Product components" where={`${specimens.length} specimens`} notes="Explore the shipped components together, grouped by category. Open a title to compare every state. Search with / and move between components with [ and ]." />
    <div className="gallery-canvas">
      {groupSpecimens(specimens).map(({ category, entries }) => <section className="gallery-overview-group" key={category} aria-labelledby={`overview-${category}`}>
        <h2 id={`overview-${category}`}>{category}<span>{entries.length}</span></h2>
        <div className="gallery-artboards">
          {entries.map((specimen) => specimen.states[0] && <StateStage key={specimen.id} state={specimen.states[0]} stage={specimen.stage} label={<a href={`#gallery/${specimen.id}`}>{specimen.title} <span>↗</span></a>} />)}
        </div>
      </section>)}
      {!specimens.length && <p>No specimens yet</p>}
    </div>
  </main></GalleryShell>;
}

function StateStage({ state, stage, label = state.label }: { state: SpecimenState; stage?: Specimen["stage"]; label?: ReactNode }) {
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  return <figure className={`gallery-artboard${stage ? ` gallery-artboard--${stage}` : ""}`} data-state-id={state.id}>
    <figcaption className="gallery-state-label">{label}</figcaption>
    <div className="gallery-stage-instance">
      {host && <PortalHostContext.Provider value={host}>{state.render()}</PortalHostContext.Provider>}
      <div ref={setHost} className="gallery-portal-host" />
    </div>
  </figure>;
}

export function GalleryDetail({ id }: { id: string }) {
  const specimen = specimens.find((entry) => entry.id === id);
  if (!specimen) return <GalleryShell><main className="gallery-page"><GalleryHeader title="Specimen not found" where="Component Gallery" /><div className="gallery-canvas"><a className="ct-bar-pill" href="#gallery">Browse components</a></div></main></GalleryShell>;
  return <GalleryShell activeId={id}><main className="gallery-page" key={specimen.id}>
    <GalleryHeader title={specimen.title} where={specimen.where} notes={specimen.notes} />
    <section className="gallery-canvas gallery-artboards" aria-label={`${specimen.title} states`}>
      {specimen.states.map((state) => <StateStage key={state.id} state={state} stage={specimen.stage} />)}
    </section>
  </main></GalleryShell>;
}
